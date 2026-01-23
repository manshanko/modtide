use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

const AUTOPATCHER: &str = "binaries/plugins/_dt_mod_autopatch.dll";
const AUTOPATCHER_TOGGLE: &str = "mods/DISABLE_AUTOPATCHER";

pub fn is_patched(darktide: &Path) -> bool {
    let path = darktide.join(AUTOPATCHER);
    if path.exists() {
        !darktide.join(AUTOPATCHER_TOGGLE).exists()
    } else {
        let path = darktide.join("bundle/bundle_database.data");
        let Ok(data) = fs::read(&path) else {
            return cfg!(debug_assertions);
        };
        bytes_check(&data, MOD_PATCH_TAG).is_some()
    }
}

pub fn toggle_patch(darktide: &Path, enable: bool) -> io::Result<()> {
    let path = darktide.join(AUTOPATCHER);
    let bundle = darktide.join("bundle");
    let autopatcher = darktide.join(AUTOPATCHER_TOGGLE);
    match (path.exists(), enable) {
        (true, true) => fs::remove_file(autopatcher),
        (true, false) => {
            fs::write(autopatcher, b"")?;
            unpatch_darktide(bundle)
        }
        (false, true) => {
            patch_darktide(bundle)?;
            match fs::remove_file(autopatcher) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err),
            }
        }
        (false, false) => unpatch_darktide(bundle),
    }
}

// from https://github.com/manshanko/dtkit-patch
const BUNDLE_DATABASE_NAME: &str = "bundle_database.data";
const BUNDLE_DATABASE_BACKUP: &str = "bundle_database.data.bak";
const BOOT_BUNDLE_NEXT_PATCH: &str = "9ba626afa44a3aa3.patch_001";
const MOD_PATCH_STARTING_POINT: [u8; 8] = u64::to_be_bytes(0xA33A4AA4AF26A69B);

const OLD_SIZE: usize = 84;
const MOD_PATCH: &[u8] = include_bytes!("./patch.bin");
const MOD_PATCH_TAG: &[u8] = b"patch_999";

fn patch_darktide(bundle_dir: PathBuf) -> io::Result<()> {
    let db_path = bundle_dir.join(BUNDLE_DATABASE_NAME);
    let mut db = fs::read(&db_path)?;

    // check if already patched for mods
    if bytes_check(&db, MOD_PATCH_TAG).is_some() {
        return Ok(());
    }

    // check for unhandled bundle patch
    if bytes_check(&db, BOOT_BUNDLE_NEXT_PATCH.as_bytes()).is_some() {
        return Err(io::Error::new(io::ErrorKind::Unsupported,
            "unexpected data in \"bundle_database.data\""));
    }

    // look for patch offset
    let Some(offset) = bytes_check(&db, &MOD_PATCH_STARTING_POINT) else {
        return Err(io::Error::new(io::ErrorKind::Unsupported,
            "could not find patch offset in \"bundle_database.data\""));
    };

    // write backup
    fs::write(bundle_dir.join(BUNDLE_DATABASE_BACKUP), &db)?;

    // insert data
    let _ = db.splice(offset..offset + OLD_SIZE, MOD_PATCH.iter().copied());

    // write patched database
    fs::write(&db_path, &db)
}

fn unpatch_darktide(bundle_dir: PathBuf) -> io::Result<()> {
    let db_path = bundle_dir.join(BUNDLE_DATABASE_NAME);
    let backup_path = bundle_dir.join(BUNDLE_DATABASE_BACKUP);

    // avoid replacing unpatched database when using `--unpatch`
    if let Ok(db) = fs::read(&db_path)
        && bytes_check(&db, MOD_PATCH_TAG).is_none()
    {
        return Ok(());
    }

    // overwrite patched database with backup database
    fs::rename(backup_path, db_path)
}

// helper function to check for slice matches
fn bytes_check(bytes: &[u8], check: &[u8]) -> Option<usize> {
    for (i, window) in bytes.windows(check.len()).enumerate() {
        if window == check {
            return Some(i);
        }
    }
    None
}
