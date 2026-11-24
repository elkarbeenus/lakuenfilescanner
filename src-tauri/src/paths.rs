use std::fs;
use std::path::PathBuf;

fn ensure(p: PathBuf) -> PathBuf {
    let _ = fs::create_dir_all(&p);
    p
}

pub fn data_dir() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| std::env::temp_dir());
    ensure(base.join("lakuenfilescanner"))
}

pub fn config_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(data_dir);
    ensure(base.join("lakuenfilescanner"))
}

pub fn db_dir() -> PathBuf {
    ensure(data_dir().join("signatures"))
}

pub fn quarantine_dir() -> PathBuf {
    ensure(data_dir().join("quarantine"))
}

pub fn runtime_dir() -> PathBuf {
    let base = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
    let p = ensure(base.join("lakuenfilescanner"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o700));
    }
    p
}
