use std::fs;
use std::fs::File;
use std::path::Path;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

use super::ArchiveReader;
use super::ArchiveList;
use super::DirEntry;
use super::FileType;
use super::Monitor;
use super::Result;

static HEADER_MAGIC_RECORD: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
static HEADER_MAGIC_FILE: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
static HEADER_MAGIC_END: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
static HEADER_DEFLATE: [u8; 2] = [0x08, 0x00];

#[allow(dead_code)]
pub struct ZipRecord<'a> {
    time: u16,
    date: u16,
    crc: u32,
    deflate_size: u32,
    size: u32,
    offset: u32,
    attr: FileType,
    name: &'a str,
}

pub struct Zip {
    file: File,
    num_records: usize,
    record_size: usize,
    record_offset: u64,
}

fn error(msg: &'static str) -> Result<()> {
    Err(io::Error::other(msg))
}

impl Zip {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut file = File::open(path)?;
        let mut buffer = [0; 64];

        file.seek(SeekFrom::End(-22))?;
        let data = &mut buffer[..22];
        file.read_exact(data)?;

        if Some(&HEADER_MAGIC_END) != data.first_chunk() {
            error("invalid zip EOCD")?;
        }
        if Some(&[0, 0]) != data[4..].first_chunk()
            || Some(&[0, 0]) != data[6..].first_chunk()
            || data[8..].first_chunk::<2>() != data[10..].first_chunk()
        {
            error("multiple zip disks not supported")?;
        }

        let num_records = u16::from_le_bytes(*data[10..].first_chunk().unwrap());
        let record_size = u32::from_le_bytes(*data[12..].first_chunk().unwrap());
        let record_offset = u32::from_le_bytes(*data[16..].first_chunk().unwrap());

        Ok(Self {
            file,
            num_records: num_records as usize,
            record_size: record_size as usize,
            record_offset: record_offset as u64,
        })
    }

    fn records(&self, mut cb: impl FnMut(&ZipRecord) -> Result<()>) -> Result<()> {
        let mut file = &self.file;
        file.seek(SeekFrom::Start(self.record_offset))?;

        let mut buffer = vec![0; self.record_size];
        file.read_exact(&mut buffer)?;

        let mut data = &buffer[..];
        for _ in 0..self.num_records {
            if data.len() < 46 {
                error("unexpected eof while parsing zip record")?;
            }

            if Some(&HEADER_MAGIC_RECORD) != data.first_chunk() {
                error("invalid zip record header")?;
            }
            if 0x14 < u16::from_le_bytes(*data[6..].first_chunk().unwrap()) {
                error("zip record is unsupported")?;
            }
            if Some(&[0, 0]) != data[8..].first_chunk() {
                error("unsupported zip record flag")?;
            }
            let method = *data[10..].first_chunk().unwrap();
            let uncompressed = [0, 0] == method;
            if !uncompressed && HEADER_DEFLATE != method {
                error("unsupported zip record compression method")?;
            }

            if Some(&[0, 0]) != data[34..].first_chunk() {
                error("invalid zip record disk")?;
            }
            let internal_attr = u16::from_le_bytes(*data[36..].first_chunk().unwrap());
            if internal_attr > 1 {
                error("unsupported zip record internal attributes")?;
            }

            let time = u16::from_le_bytes(*data[12..].first_chunk().unwrap());
            let date = u16::from_le_bytes(*data[14..].first_chunk().unwrap());
            let crc = u32::from_le_bytes(*data[16..].first_chunk().unwrap());
            let deflate_size = u32::from_le_bytes(*data[20..].first_chunk().unwrap());
            let size = u32::from_le_bytes(*data[24..].first_chunk().unwrap());
            let name_len = u16::from_le_bytes(*data[28..].first_chunk().unwrap());
            let extra_len = u16::from_le_bytes(*data[30..].first_chunk().unwrap());
            let comment_len = u16::from_le_bytes(*data[32..].first_chunk().unwrap());
            let attr = u32::from_le_bytes(*data[38..].first_chunk().unwrap());
            let offset = u32::from_le_bytes(*data[42..].first_chunk().unwrap());

            let ty = match attr & 0xff {
                0x10 => FileType::Dir,
                0x20 => FileType::File,
                _ => return error("unknown file type in zip record"),
            };

            let name_len = name_len as usize;
            let extra_len = extra_len as usize;
            let comment_len = comment_len as usize;
            let record_len = 46 + name_len + extra_len + comment_len;
            if data.len() < record_len {
                error("unexpected eof while parsing zip record name")?;
            }

            let name = std::str::from_utf8(&data[46..46 + name_len]).unwrap();
            if !name.is_ascii() {
                error("only ascii names are supported in zip record")?;
            }

            cb(&ZipRecord {
                time,
                date,
                crc,
                deflate_size,
                size,
                offset,
                attr: ty,
                name: name.strip_suffix("/").unwrap_or(name),
            })?;

            data = &data[record_len..];
        }

        Ok(())
    }

    fn read_record<'a>(
        &self,
        record: &ZipRecord,
        buffer: &'a mut Vec<u8>,
    ) -> Result<&'a [u8]> {
        let deflate_size = record.deflate_size as usize;
        let size = record.size as usize;
        let size_needed = deflate_size + size + 0x100;
        if buffer.len() < size_needed {
            buffer.resize(size_needed, 0);
        }

        let offset = record.offset as usize;
        let mut file = &self.file;
        file.seek(SeekFrom::Start(offset as u64))?;
        file.read_exact(&mut buffer[..30])?;
        let data = &buffer[..];
        if Some(&HEADER_MAGIC_FILE) != data.first_chunk() {
            error("invalid zip file header")?;
        }

        let method = u16::from_le_bytes(*data[8..].first_chunk().unwrap());
        if method != 0 && method != 8 {
            error("unsupported zip file compression method")?;
        }

        let crc = u32::from_le_bytes(*data[14..].first_chunk().unwrap());
        if crc != record.crc {
            error("failed to verify zip file header")?;
        }

        let name_len = u16::from_le_bytes(*data[26..].first_chunk().unwrap());
        let extra_len = u16::from_le_bytes(*data[28..].first_chunk().unwrap());
        let offset = offset + 30 + name_len as usize + extra_len as usize;

        let (data, out) = buffer.split_at_mut(deflate_size + deflate_size % 16);
        let data = &mut data[..deflate_size];

        file.seek(SeekFrom::Start(offset as u64))?;
        file.read_exact(data)?;
        if method == 0 {
            return Ok(data);
        }

        let out = &mut out[..size];
        let len = miniz_oxide::inflate::decompress_slice_iter_to_slice(
            out,
            [&*data].into_iter(),
            false,
            true,
        ).unwrap();
        assert!(len == size);
        Ok(out)
    }
}

impl ArchiveReader for Zip {
    fn list(&self, monitor: &Monitor) -> Result<ArchiveList> {
        let mut entries = Vec::new();
        let mut total = 0;
        let mut first = true;
        self.records(|record| {
            monitor.stopped()?;

            total += record.size as u64;
            if total > u32::MAX as u64 {
                return Err(io::Error::other("zip output larger than supported"));
            }

            if first && let Some((root, _)) = record.name.split_once('/') {
                entries.push(DirEntry::new(root, FileType::Dir));
            }
            first = false;
            entries.push(DirEntry::new(record.name, record.attr));
            Ok(())
        })?;
        Ok(ArchiveList::new(entries))
    }

    fn copy(&self, monitor: &Monitor, dest: &Path) -> Result<()> {
        let mut buffer = Vec::new();
        let mut total = 0;
        let mut first = true;
        self.records(|record| {
            monitor.stopped()?;

            if first && let Some((root, _)) = record.name.split_once('/')
                && let Err(err) = fs::create_dir(dest.join(root))
                && err.kind() != io::ErrorKind::AlreadyExists
            {
                return Err(err);
            }
            first = false;

            if record.attr.is_dir() {
                if let Err(err) = fs::create_dir(dest.join(record.name))
                    && err.kind() != io::ErrorKind::AlreadyExists
                {
                    return Err(err);
                }
            } else if record.attr.is_file() {
                let data = self.read_record(record, &mut buffer)?;

                total += data.len() as u64;
                if total > u32::MAX as u64 {
                    return Err(io::Error::other("zip output larger than supported"));
                }

                fs::write(dest.join(record.name), data)?;
            }
            Ok(())
        })
    }
}
