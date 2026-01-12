use std::ffi::OsStr;
use std::fs;
use std::fmt::Write;
use std::path::Path;
use std::path::PathBuf;

pub struct ModEngine {
    pub header: String,
    pub mods: Vec<ModEntry>
}

impl ModEngine {
    pub fn new() -> Self {
        Self {
            header: String::new(),
            mods: Vec::new(),
        }
    }

    pub fn scan(path: impl AsRef<Path>) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        let mut out = Vec::new();
        let path = path.as_ref();
        for fd in fs::read_dir(&path)? {
            let dir = match fs::read_dir(fd?.path()) {
                Ok(fd) => fd,
                Err(err) if err.kind() == std::io::ErrorKind::NotADirectory => continue,
                Err(err) => return Err(err.into()),
            };

            let mut ext = None;
            for fd in dir {
                let p = fd?.path();
                if p.extension() != Some(OsStr::new("mod")) {
                    continue;
                } else if ext.is_some() {
                    ext = None;
                    break;
                }

                if let Ok(p) = p.strip_prefix(path) {
                    ext = Some(p.to_path_buf());
                }
            }

            if let Some(ext) = ext
                && let Some(sort) = ext.file_stem()
            {
                let lower = sort.to_string_lossy().to_lowercase();
                out.push((ext, lower));
            }
        }
        out.sort_by(|(_, a), (_, b)| a.cmp(&b));
        Ok(out.into_iter().map(|(p, _)| p).collect())
    }

    pub fn load(
        &mut self,
        load_order: &str,
        paths: &[PathBuf],
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.header.clear();
        self.mods.clear();

        let mut in_comments = true;
        for line in load_order.lines() {
            if in_comments && line.starts_with("-- ") {
                self.header.push_str(line);
                self.header.push('\n');
                continue;
            } else {
                in_comments = false;
            }

            if line.is_empty() {
                continue;
            }

            let mut state = ModState::Enabled;
            let mut name = line;
            if let Some(m) = line.strip_prefix("--") {
                state = ModState::Disabled;
                name = m.trim_start();
            }

            if name == "base" || name == "dmf" {
                continue;
            }

            self.mods.push(ModEntry {
                state,
                name: name.to_string(),
                path: PathBuf::new(),
            });
        }

        for path in paths {
            if path == "base/base.mod" || path == "dmf/dmf.mod" {
                continue;
            }

            let mut iter = path.iter();
            let mut path = None;
            let mut file = None;
            while let Some(c) = iter.next() {
                if c == OsStr::new(".") {
                    continue;
                }
                path = Some(c);
                file = iter.next();
            }

            if file.is_none() || iter.next().is_some() {
                continue;
            }
            let path = path.unwrap();
            let file = file.unwrap();

            let Some(name) = file.to_str() else {
                continue;
            };

            let Some(name) = name.strip_suffix(".mod") else {
                continue;
            };

            let disabled = path != name;
            if disabled
                && let Some(p) = path.to_str()
                && let Some(p) = p.strip_prefix("_")
                && p != name
            {
                continue;
            }

            let m = self.mods.iter_mut()
                .find(|m| m.name == name);

            if let Some(m) = m {
                m.path = PathBuf::from(path);
                if disabled {
                    m.state = ModState::Disabled;
                }
            } else {
                let state = if disabled {
                    ModState::Disabled
                } else {
                    ModState::MissingEntry
                };
                self.mods.push(ModEntry {
                    state,
                    name: name.to_string(),
                    path: PathBuf::from(path),
                });
            }
        }

        for m in &mut self.mods {
            if *m.path == *"" {
                m.state = ModState::NotInstalled;
            }
        }

        Ok(())
    }

    pub fn generate(&self, out: &mut String) -> Result<(), Box<dyn std::error::Error>> {
        out.push_str(&self.header);
        for m in &self.mods {
            match m.state {
                ModState::Enabled => (),
                ModState::Disabled
                | ModState::NotInstalled => write!(out, "--")?,
                ModState::MissingEntry => continue,
            }
            writeln!(out, "{}", m.name)?;
        }
        Ok(())
    }
}

pub struct ModEntry {
    pub state: ModState,
    name: String,
    path: PathBuf,
}

impl ModEntry {
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModState {
    Enabled,
    Disabled,
    MissingEntry,
    NotInstalled,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn state() {
        use ModState::*;

        let header = "-- line1\n-- line2\nbase\ndmf\n--dmf\n";
        let test: &[(&str, &str, ModState)] = &[
            ("on1", "on1/on1.mod", Enabled),
            ("on2", "./on2/on2.mod", Enabled),
            ("--off1", "off1/off1.mod", Disabled),
            ("off2", "_off2/off2.mod", Disabled),
            ("not_ins1", "", NotInstalled),
            ("not_ins2", "__not_ins2/not_ins2.mod", NotInstalled),
            ("", "miss_ent1/miss_ent1.mod", MissingEntry),
            ("", "_miss_ent2/miss_ent2.mod", Disabled),
        ];

        let mut load_order = String::from(header);
        for (name, ..) in test {
            load_order.push_str(name);
            load_order.push('\n');
        }

        let mut paths = Vec::new();
        for (_, path, ..) in test {
            paths.push(PathBuf::from(path));
        }

        let mut engine = ModEngine::new();
        engine.load(&load_order, &paths).unwrap();
        for (m, t) in engine.mods.iter().zip(test.iter()) {
            let name = t.1
                .split("/")
                .nth(1)
                .and_then(|s| s.strip_suffix(".mod"))
                .unwrap_or(t.0);

            assert_eq!(m.name, name);
            assert_eq!(m.state, t.2, "{name}");
        }
    }
}
