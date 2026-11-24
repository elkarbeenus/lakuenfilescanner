use crate::scanner::ScanSummary;
use crate::{paths, util};
use std::path::PathBuf;
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());
const MAX: usize = 60;

fn file() -> PathBuf {
    paths::data_dir().join("history.json")
}

pub fn list() -> Vec<ScanSummary> {
    util::read_json(&file()).unwrap_or_default()
}

pub fn add(s: &ScanSummary) {
    let _g = LOCK.lock().unwrap();
    let mut v = list();
    let mut s = s.clone();
    s.error_list.truncate(300);
    v.insert(0, s);
    v.truncate(MAX);
    let _ = util::write_json(&file(), &v);
}

pub fn get(id: &str) -> Option<ScanSummary> {
    list().into_iter().find(|s| s.id == id)
}

pub fn set_status(scan_id: &str, finding_id: &str, status: &str) {
    let _g = LOCK.lock().unwrap();
    let mut v = list();
    let mut changed = false;
    for s in v.iter_mut().filter(|s| s.id == scan_id) {
        for f in s.findings.iter_mut().filter(|f| f.id == finding_id) {
            f.status = status.to_string();
            changed = true;
        }
    }
    if changed {
        let _ = util::write_json(&file(), &v);
    }
}

pub fn remove(id: &str) {
    let _g = LOCK.lock().unwrap();
    let mut v = list();
    v.retain(|s| s.id != id);
    let _ = util::write_json(&file(), &v);
}

pub fn clear() {
    let _g = LOCK.lock().unwrap();
    let _ = std::fs::remove_file(file());
}
