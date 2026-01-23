use std::ffi::OsStr;
use std::fs;
use std::io;
use std::io::Result;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;

mod raw;
use raw::RawDir;
mod zip;
use zip::Zip;

trait ArchiveReader: Send + Sync {
    fn list(&self, monitor: &Monitor) -> Result<ArchiveList>;
    fn copy(&self, monitor: &Monitor, dest: &Path) -> Result<()>;
}

fn open_archive(path: &Path) -> Result<Option<Box<dyn ArchiveReader>>> {
    let meta = fs::metadata(path)?;
    if meta.is_dir() {
        Ok(Some(Box::new(RawDir::new(path)?)))
    } else if !meta.is_file() {
        Ok(None)
    } else if Some(OsStr::new("zip")) == path.extension() {
        Ok(Some(Box::new(Zip::new(path)?)))
    } else {
        // TODO: more archive formats
        Ok(None)
    }
}

struct Monitor(AtomicBool);

impl Monitor {
    fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn stopped(&self) -> Result<()> {
        if self.0.load(Ordering::SeqCst) {
            Err(io::Error::new(io::ErrorKind::WouldBlock, "operation canceled"))
        } else {
            Ok(())
        }
    }
}

fn entry_cmp_(
    ap: &str,
    ak: FileType,
    bp: &str,
    bk: FileType,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let mut ap = ap.split('/');
    let mut bp = bp.split('/');
    let ac = ap.clone().count();
    let bc = bp.clone().count();

    let mut ord = std::cmp::Ordering::Equal;
    let mut prefix_match = true;
    let mut checked = 0;
    while let (Some(a), Some(b)) = (ap.next(), bp.next()) {
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

        let shorter = a.next().cmp(&b.next());
        ord = ord.then(shorter);

        checked += 1;
        if ord.is_ne() {
            prefix_match = false;
            break;
        }
    }

    let count = ac.cmp(&bc);
    let kind = ak.cmp(&bk);

    if count.is_eq() && checked == ac {
        kind.then(ord)
    } else if prefix_match {
        count
    } else if checked == ac || checked == bc {
        let prio = match count {
            Ordering::Less => ak.cmp(&FileType::Dir),
            Ordering::Equal => ak.cmp(&bk),
            Ordering::Greater => FileType::Dir.cmp(&bk),
        };
        prio.then(ord)
    } else {
        ord.then(count)
    }
}

fn entry_cmp(a: &DirEntry, b: &DirEntry) -> std::cmp::Ordering {
    entry_cmp_(&a.path, a.kind, &b.path, b.kind)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileType {
    Dir,
    File,
}

impl FileType {
    pub fn is_file(&self) -> bool {
        matches!(*self, FileType::File)
    }

    pub fn is_dir(&self) -> bool {
        matches!(*self, FileType::Dir)
    }
}

#[derive(PartialEq)]
pub struct DirEntry {
    kind: FileType,
    path: String,
}

impl DirEntry {
    fn new(path: &str, kind: FileType) -> Self {
        assert!(!path.contains(".."));
        Self {
            kind,
            path: path.replace('\\', "/"),
        }
    }
}

pub struct ArchiveList<T = Vec<DirEntry>> {
    entries: T,
    offset: usize,
}

impl ArchiveList {
    fn new(mut entries: Vec<DirEntry>) -> Self {
        entries.sort_by(entry_cmp);
        Self {
            entries,
            offset: 0,
        }
    }

    fn compose(lists: Vec<ArchiveList>) -> Self {
        let mut entries = Vec::new();
        for list in lists {
            for entry in list.entries {
                entries.push(entry);
            }
        }

        entries.sort_by(entry_cmp);
        let mut prev: Option<&DirEntry> = None;
        for entry in &entries {
            if let Some(prev) = prev
                && entry.kind != prev.kind
                && entry.path == prev.path
            {
                panic!("conflict: {:?}", entry.path);
            }
            prev = Some(entry);
        }
        entries.dedup();

        Self {
            entries,
            offset: 0,
        }
    }
}

impl<T: AsRef<[DirEntry]>> ArchiveList<T> {
    pub fn list(&self, key: &str) -> Option<ArchiveList<&[DirEntry]>> {
        let e = self.entries.as_ref();
        let o = self.offset;
        if let Ok(start) = e.binary_search_by(|p| entry_cmp_(&p.path[o..], p.kind, key, FileType::Dir))
            && e[start].kind.is_dir()
        {
            let end = start + e[start..].partition_point(|p| entry_cmp_(&p.path[o..], p.kind, key, FileType::Dir).is_ge());
            let start = end.min(start + 1);
            Some(ArchiveList {
                entries: &e[start..end],
                offset: o + key.len() + 1,
            })
        } else {
            None
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, FileType, usize)> {
        let e = self.entries.as_ref();
        e.iter()
            .map(|entry| {
                let path = &entry.path[self.offset..];
                let mut iter = path.split('/');
                let mut last = iter.next().expect("internal error from sorting ArchiveList");
                let mut depth = 0;
                for part in iter {
                    last = part;
                    depth += 1;
                }
                (last, entry.kind, depth)
            })
    }
}

#[derive(Clone)]
pub enum Prefix {
    None,
    Mods,
}

impl Prefix {
    fn prepend(&self, list: &mut ArchiveList) {
        let prefix = match *self {
            Prefix::None => return,
            Prefix::Mods => "mods/",
        };

        for entry in &mut list.entries {
            entry.path.insert_str(0, prefix);
        }

        let parent = prefix.strip_suffix("/").unwrap();
        list.entries.insert(0, DirEntry::new(parent, FileType::Dir));
    }
}

struct ArchiveInner {
    monitor: Monitor,
    archives: Vec<(PathBuf, Box<dyn ArchiveReader>)>,
    fixup: fn(&Path, &ArchiveList) -> Result<Prefix>,
}

pub struct Archive(Arc<ArchiveInner>);

impl Archive {
    pub fn new(
        paths: &[PathBuf],
        fixup: fn(&Path, &ArchiveList) -> Result<Prefix>,
    ) -> Result<Self> {
        let mut archives = Vec::with_capacity(paths.len());
        for path in paths {
            let archive = open_archive(path)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotADirectory,
                    "failed to find valid archive"))?;
            archives.push((path.to_path_buf(), archive));
        }
        Ok(Archive(Arc::new(ArchiveInner {
            monitor: Monitor(AtomicBool::new(false)),
            archives,
            fixup,
        })))
    }

    pub fn view(&self, complete: impl FnOnce(Result<ArchiveView>) + Send + 'static) {
        let dispatch = self.0.clone();
        thread::spawn(move || {
            let d = &dispatch;
            let fixup = d.fixup;
            let mut lists = Vec::new();
            let mut prefixes = Vec::new();
            for (p, rdr) in &d.archives {
                let mut list = match rdr.list(&d.monitor) {
                    Ok(list) => list,
                    Err(err) => {
                        complete(Err(err));
                        return;
                    }
                };

                let prefix = match fixup(p, &list) {
                    Ok(p) => p,
                    Err(err) => {
                        complete(Err(err));
                        return;
                    }
                };
                prefix.prepend(&mut list);
                prefixes.push(prefix);
                lists.push(list);
            }
            let list = ArchiveList::compose(lists);
            complete(Ok(ArchiveView {
                inner: dispatch,
                prefixes,
                list,
                copied: false,
            }));
        });
    }
}

impl Drop for Archive {
    fn drop(&mut self) {
        self.0.monitor.cancel();
    }
}

pub struct ArchiveView {
    inner: Arc<ArchiveInner>,
    prefixes: Vec<Prefix>,
    list: ArchiveList,
    copied: bool,
}

impl ArchiveView {
    pub fn list(&self) -> &ArchiveList {
        &self.list
    }

    pub fn copy(&mut self, dest: &Path, complete: impl FnOnce(Result<u64>) + Send + 'static) {
        assert!(!self.copied);
        self.copied = true;

        assert!(self.prefixes.len() == self.inner.archives.len());
        let prefixes = core::mem::take(&mut self.prefixes);
        let inner = self.inner.clone();

        let dest = dest.to_path_buf();
        thread::spawn(move || {
            let mut mods_exists = false;
            let mut count = 0;
            for (i, prefix) in prefixes.iter().enumerate() {
                let rdr = &inner.archives[i].1;

                let _owner;
                let path = match prefix {
                    Prefix::None => &dest,
                    Prefix::Mods => {
                        _owner = dest.join("mods");
                        if !mods_exists {
                            mods_exists = true;
                            let _ = fs::create_dir(&dest);
                        }
                        &_owner
                    }
                };

                if let Err(err) = rdr.copy(&inner.monitor, path) {
                    complete(Err(err));
                    return;
                }
                count += 1;
            }
            complete(Ok(count));
        });
    }
}
