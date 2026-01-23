use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::fmt::Write;
use std::path::Path;

pub struct ModEngine {
    pub header: String,
    pub mods: Vec<ModEntry>,
}

impl ModEngine {
    pub fn new() -> Self {
        Self {
            header: String::new(),
            mods: Vec::new(),
        }
    }

    pub fn scan(path: impl AsRef<Path>) -> Result<Vec<Metadata>, Box<dyn std::error::Error>> {
        let mut out = Vec::new();
        let path = path.as_ref();
        for fd in fs::read_dir(path)? {
            let dir = match fs::read_dir(fd?.path()) {
                Ok(fd) => fd,
                Err(err) if err.kind() == std::io::ErrorKind::NotADirectory => continue,
                Err(err) => return Err(err.into()),
            };

            let mut meta = None;
            for fd in dir {
                let file_path = fd?.path();
                if file_path.extension() != Some(OsStr::new("mod")) {
                    continue;
                }

                if let Ok(p) = file_path.strip_prefix(path)
                    && p.file_stem() == p.parent().map(|p| p.as_os_str())
                    && let Some(name) = p.to_str()
                    && let Ok(file) = fs::read_to_string(&file_path)
                {
                    meta = Some(Metadata::fuzzy_parse_mod(name, &file));
                    break;
                }
            }

            if let Some(meta) = meta {
                out.push(meta);
            }
        }
        Ok(out)
    }

    pub fn load(
        &mut self,
        load_order: &str,
        found: Vec<Metadata>,
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
                meta: Metadata::new(""),
                state,
                name: name.to_string(),
            });
        }

        for meta in found {
            let Some(name) = meta.name() else {
                continue;
            };
            if name == "base" || name == "dmf" {
                continue;
            }

            let m = self.mods.iter_mut()
                .find(|m| m.name == name);

            if let Some(m) = m {
                m.meta = meta;
            } else {
                self.mods.push(ModEntry {
                    state: ModState::MissingEntry,
                    name: name.to_string(),
                    meta,
                });
            }
        }

        for m in &mut self.mods {
            if *m.path() == *"" {
                m.state = ModState::NotInstalled;
            }
        }

        Ok(())
    }

    pub fn sort(&mut self) -> Option<Vec<(String, String)>> {
        let mut dag: HashMap<&str, Vec<&str>> = self.mods.iter()
            .map(|m| (m.name.as_str(), Vec::new()))
            .collect();

        let mut used = HashSet::new();

        let mut missing = Vec::new();
        for m in &self.mods {
            let meta = &m.meta;
            for name in &meta.require {
                if !dag.contains_key(name.as_str()) {
                    missing.push((m.name.to_string(), name.to_string()));
                }
            }
        }

        for m in &self.mods {
            let meta = &m.meta;
            if meta.load_before.is_empty()
                && meta.load_after.is_empty()
                && meta.require.is_empty()
            {
                continue;
            } else {
                used.insert(m.name.as_str());
            }

            for name in &meta.load_before {
                let Some(entry) = dag.get_mut(name.as_str()) else {
                    continue;
                };
                if let Err(i) = entry.binary_search(&name.as_str()) {
                    used.insert(name.as_str());
                    entry.insert(i, &m.name);
                }
            }

            let entry = dag.get_mut(m.name.as_str()).unwrap();
            for name in &meta.load_after {
                if let Err(i) = entry.binary_search(&name.as_str()) {
                    used.insert(name.as_str());
                    entry.insert(i, name);
                }
            }
            for name in &meta.require {
                if !meta.load_before.contains(name)
                    && let Err(i) = entry.binary_search(&name.as_str())
                {
                    used.insert(name.as_str());
                    entry.insert(i, name);
                }
            }
        }

        let mut queue = Vec::with_capacity(self.mods.len());
        let mut order = Vec::with_capacity(self.mods.len());
        for (i, m) in self.mods.iter().enumerate() {
            if used.contains(m.name.as_str()) {
                queue.push(Some(m.name.as_str()));
            } else {
                queue.push(None);
                dag.remove(m.name.as_str());
                order.push((u32::MAX, i));
            }
        }

        let mut round = 0;
        let mut offset = usize::MAX;
        while offset != order.len() {
            offset = order.len();
            for (i, name_) in queue.iter_mut().enumerate() {
                let Some(name) = name_ else {
                    continue;
                };

                let mut resolved = true;
                if let Some(lb_list) = dag.get(name) {
                    for lb in lb_list {
                        if dag.contains_key(lb) {
                            resolved = false;
                            break;
                        }
                    }
                }
                if resolved {
                    order.push((round, i));
                    *name_ = None;
                }
            }

            for (_, i) in &order[offset..] {
                let name = &self.mods[*i].name;
                dag.remove(name.as_str());
            }

            round += 1;
        }

        if offset != queue.len() {
            return None;
        }

        order.sort_by(|a, b| {
            let mut ord = a.0.cmp(&b.0);
            if ord.is_eq() {
                let a = &self.mods[a.1].name;
                let b = &self.mods[b.1].name;

                let mut a = a.as_bytes().iter();
                let mut b = b.as_bytes().iter();
                while let (Some(a), Some(b)) = (a.next(), b.next()) {
                    let a = a.to_ascii_lowercase();
                    let b = b.to_ascii_lowercase();
                    ord = a.cmp(&b);
                    if ord.is_ne() {
                        break;
                    }
                }
            }
            ord
        });

        let mut mods = Vec::with_capacity(self.mods.len());
        for m in self.mods.drain(..) {
            mods.push(Some(m));
        }

        for (_, i) in order {
            self.mods.push(mods[i].take().unwrap());
        }

        Some(missing)
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

pub struct Metadata {
    path: String,
    load_before: Vec<String>,
    load_after: Vec<String>,
    require: Vec<String>,
    #[allow(dead_code)]
    version: Option<String>,
}

impl Metadata {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.replace('\\', "/"),
            load_before: Vec::new(),
            load_after: Vec::new(),
            require: Vec::new(),
            version: None,
        }
    }

    fn parse_value(text: &str) -> Option<Result<String, Vec<String>>> {
        let text = text.trim_start()
            .strip_prefix('=')?
            .trim_start();

        if let Some(text) = text.strip_prefix('"') {
            let (name, _) = text.split_once('"')?;
            Some(Ok(name.to_string()))
        } else if let Some(mut text) = text.strip_prefix('{') {
            text = text.trim_start();
            let mut list = Vec::new();
            while !text.starts_with('}') {
                text = text.strip_prefix('"')?;
                let name;
                (name, text) = text.split_once('"')?;
                list.push(name.to_string());
                text = text.trim_start()
                    .strip_prefix(",")
                    .unwrap_or(text)
                    .trim_start();
            }
            Some(Err(list))
        } else {
            None
        }
    }

    fn find_key_value(file: &str, key: &str) -> Option<Result<String, Vec<String>>> {
        let mut offset = 0;
        while let Some(offset_) = file[offset..].find(key) {
            offset += offset_ + key.len();
            if let Some(res) = Self::parse_value(&file[offset..]) {
                return Some(res);
            }
        }
        None
    }

    pub fn fuzzy_parse_mod(path: &str, file: &str) -> Self {
        let mut load_before = Vec::new();
        let mut load_after = Vec::new();
        let mut require = Vec::new();
        let mut version = None;

        if let Some(Err(list)) = Self::find_key_value(file, "load_before") {
            load_before = list;
        }

        if let Some(Err(list)) = Self::find_key_value(file, "load_after") {
            load_after = list;
        }

        if let Some(Err(list)) = Self::find_key_value(file, "require") {
            require = list;
        }

        if let Some(Ok(value)) = Self::find_key_value(file, "version") {
            version = Some(value);
        }

        Self {
            path: path.replace('\\', "/"),
            load_before,
            load_after,
            require,
            version,
        }
    }

    pub fn name(&self) -> Option<&str> {
        self.path.split_once('/').and_then(|(_, name)| name.strip_suffix(".mod"))
    }
}

pub struct ModEntry {
    pub meta: Metadata,
    pub state: ModState,
    name: String,
}

impl ModEntry {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &str {
        &self.meta.path
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
            //("on2", "./on2/on2.mod", Enabled),
            ("--off1", "off1/off1.mod", Disabled),
            //("off2", "_off2/off2.mod", Disabled),
            ("not_ins1", "", NotInstalled),
            //("not_ins2", "__not_ins2/not_ins2.mod", NotInstalled),
            ("", "miss_ent1/miss_ent1.mod", MissingEntry),
            //("", "_miss_ent2/miss_ent2.mod", Disabled),
        ];

        let mut load_order = String::from(header);
        for (name, ..) in test {
            load_order.push_str(name);
            load_order.push('\n');
        }

        let mut metas = Vec::new();
        for (_, path, ..) in test {
            metas.push(Metadata::new(path));
        }

        let mut engine = ModEngine::new();
        engine.load(&load_order, metas).unwrap();
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

    #[test]
    fn sort() {
        let expected: &[&str] = &[
            "abc",
            "load_before1",
            "bca",
            "load_before2",
            "requires",
            "late",
            "aaa",
        ];
        let test: &[(&str, &str)] = &[
            ("aaa", ""),
            ("abc", ""),
            ("bca", ""),
            ("requires", "require = {\"bca\"}"),
            ("load_before1", "load_before = {\"bca\"}"),
            ("load_before2", "load_after = {\"bca\"} require = {\"abc\"}"),
            ("late", "require = {\"requires\"}"),
        ];

        let mut metas = Vec::new();
        for (name, file) in test {
            let path = format!("{name}/{name}.mod");
            metas.push(Metadata::fuzzy_parse_mod(&path, file));
        }

        let mut engine = ModEngine::new();
        engine.load("", metas).unwrap();
        let missing = engine.sort().unwrap();
        assert!(missing.is_empty());

        let mut failed = None;
        for i in 0..test.len() {
            let a = &engine.mods[i].name;
            let b = expected[i];
            if a != b {
                failed = Some((a, b));
                break;
            }
        }

        if let Some((a, b)) = failed {
            for i in 0..test.len() {
                let a = &engine.mods[i].name;
                let b = expected[i];
                eprintln!("{a}, {b}");
            }
            panic!("{a} != {b}");
        }
    }

    #[test]
    fn sort_fail() {
        let test: &[(&str, &str)] = &[
            ("aa", "load_before = {\"bb\"}"),
            ("bb", ""),
            ("ba", "require = {\"bb\"} load_before = {\"aa\"}"),
        ];

        let mut metas = Vec::new();
        for (name, file) in test {
            let path = format!("{name}/{name}.mod");
            metas.push(Metadata::fuzzy_parse_mod(&path, file));
        }

        let mut engine = ModEngine::new();
        engine.load("", metas).unwrap();
        assert!(engine.sort().is_none());
    }

    #[test]
    fn sort_missing_require() {
        let test: &[(&str, &str)] = &[
            ("a", "require = {\"b\"}"),
        ];

        let mut metas = Vec::new();
        for (name, file) in test {
            let path = format!("{name}/{name}.mod");
            metas.push(Metadata::fuzzy_parse_mod(&path, file));
        }

        let mut engine = ModEngine::new();
        engine.load("", metas).unwrap();
        assert_eq!(1, engine.sort().unwrap().len());
    }
}
