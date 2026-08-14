use std::{fs, io, path::Path};

pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes)?;
    if path.exists() { fs::remove_file(path)?; }
    fs::rename(temporary, path)
}
