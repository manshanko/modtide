use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::io;

use super::ArchiveReader;
use super::ArchiveList;
use super::DirEntry;
use super::FileType;
use super::Monitor;
use super::Result;

pub struct RawDir {
    path: PathBuf,
}

impl RawDir {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().canonicalize()?;
        if path.metadata()?.is_dir() {
            Ok(Self {
                path,
            })
        } else {
            Err(io::Error::new(io::ErrorKind::NotADirectory, "RawDir requires valid directory"))
        }
    }

    fn iter_all(
        &self,
        mut cb: impl FnMut(&Path, &Path, FileType) -> Result<()>,
    ) -> Result<()> {
        let name = Path::new(self.path.file_name().unwrap());
        cb(&self.path, name, FileType::Dir)?;

        let mut next = fs::read_dir(&self.path)?;
        let mut iter = Vec::new();
        loop {
            for fd in next {
                let fd = fd?;
                let path = fd.path();
                let suffix = path.strip_prefix(self.path.parent().unwrap()).unwrap();
                let type_ = match fd.file_type()? {
                    ty if ty.is_file() => FileType::File,
                    ty if ty.is_dir() => FileType::Dir,
                    _ => todo!(),
                };
                cb(&path, suffix, type_)?;
                if type_.is_dir() {
                    iter.push(path);
                }
            }

            let Some(path) = iter.pop() else {
                break;
            };
            next = fs::read_dir(path)?;
        }
        Ok(())
    }
}

impl ArchiveReader for RawDir {
    fn list(&self, monitor: &Monitor) -> Result<ArchiveList> {
        let mut entries = Vec::new();
        self.iter_all(|_path, suffix, type_| {
            monitor.stopped()?;

            let suffix = suffix.to_string_lossy();
            entries.push(DirEntry::new(&suffix, type_));
            Ok(())
        })?;
        Ok(ArchiveList::new(entries))
    }

    fn copy(&self, monitor: &Monitor, dest: &Path) -> Result<()> {
        self.iter_all(|path, suffix, type_| {
            monitor.stopped()?;

            if type_.is_dir() {
                if let Err(err) = fs::create_dir(dest.join(suffix))
                    && err.kind() != io::ErrorKind::AlreadyExists
                {
                    return Err(err);
                }
            } else if type_.is_file() {
                fs::copy(path, dest.join(suffix))?;
            }
            Ok(())
        })
    }
}
