//! Quarantine: files are moved into a private folder, XOR-encoded so they
//! cannot be executed or opened by accident, and can be restored any time.

use crate::{paths, util};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const KEY: u8 = 0xA5;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QItem {
    pub id: String,
    pub original_path: String,
    pub signature: String,
    pub severity: String,
    pub size: u64,
    pub sha256: String,
    pub mode: u32,
    pub quarantined_at: u64,
}

fn index_file() -> PathBuf {
    paths::quarantine_dir().join("index.json")
}

pub fn list() -> Vec<QItem> {
    let mut v: Vec<QItem> = util::read_json(&index_file()).unwrap_or_default();
    v.sort_by(|a, b| b.quarantined_at.cmp(&a.quarantined_at));
    v
}

fn save(v: &[QItem]) -> Result<(), String> {
    util::write_json(&index_file(), &v)
}

fn copy_xor(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut i = File::open(src)?;
    let mut o = File::create(dst)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = i.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for b in &mut buf[..n] {
            *b ^= KEY;
        }
        o.write_all(&buf[..n])?;
    }
    o.sync_all()?;
    Ok(())
}

pub fn add(path: &Path, signature: &str, severity: &str) -> Result<QItem, String> {
    let md = fs::metadata(path).map_err(|e| e.to_string())?;
    let sha256 = util::sha256_file(path).map_err(|e| e.to_string())?;
    let id = util::new_id();
    let dst = paths::quarantine_dir().join(format!("{id}.lqf"));
    copy_xor(path, &dst).map_err(|e| {
        let _ = fs::remove_file(&dst);
        e.to_string()
    })?;
    let _ = fs::set_permissions(&dst, fs::Permissions::from_mode(0o600));
    if let Err(e) = fs::remove_file(path) {
        let _ = fs::remove_file(&dst);
        return Err(e.to_string());
    }
    let item = QItem {
        id,
        original_path: path.display().to_string(),
        signature: signature.into(),
        severity: severity.into(),
        size: md.len(),
        sha256,
        mode: md.permissions().mode() & 0o7777,
        quarantined_at: util::now_ms(),
    };
    let mut v = list();
    v.push(item.clone());
    save(&v)?;
    Ok(item)
}

/// Restore to the original location. Returns the path written.
pub fn restore(id: &str) -> Result<String, String> {
    let mut v = list();
    let pos = v.iter().position(|q| q.id == id).ok_or("not found")?;
    let item = v[pos].clone();
    let src = paths::quarantine_dir().join(format!("{id}.lqf"));
    let mut dst = PathBuf::from(&item.original_path);
    if dst.exists() {
        dst = PathBuf::from(format!("{}.restored", item.original_path));
    }
    if let Some(p) = dst.parent() {
        fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    copy_xor(&src, &dst).map_err(|e| e.to_string())?;
    let _ = fs::set_permissions(&dst, fs::Permissions::from_mode(item.mode));
    let _ = fs::remove_file(&src);
    v.remove(pos);
    save(&v)?;
    Ok(dst.display().to_string())
}

pub fn delete(id: &str) -> Result<(), String> {
    let mut v = list();
    let src = paths::quarantine_dir().join(format!("{id}.lqf"));
    let _ = fs::remove_file(&src);
    v.retain(|q| q.id != id);
    save(&v)
}

pub fn sha_of(id: &str) -> Option<QItem> {
    list().into_iter().find(|q| q.id == id)
}
