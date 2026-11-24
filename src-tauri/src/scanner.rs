//! Parallel scanner: one walker feeds N workers. Each worker checks Lakuen
//! rules and asks the private clamd (signatures loaded once in RAM).

use crate::{engine, heuristics, paths, util};
use crossbeam_channel::bounded;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

#[derive(Default)]
pub struct ScanControl {
    pub cancel: AtomicBool,
    pub paused: AtomicBool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Finding {
    pub id: String,
    pub path: String,
    pub signature: String,
    /// infected | pua | suspicious
    pub severity: String,
    /// clamav | lakuen
    pub engine: String,
    pub size: u64,
    /// open | quarantined | deleted | ignored
    pub status: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ErrorEntry {
    pub path: String,
    pub message: String,
}

#[derive(Serialize, Clone, Default, Debug)]
pub struct Progress {
    pub scan_id: String,
    pub running: bool,
    pub paused: bool,
    pub cancelled: bool,
    pub walk_done: bool,
    pub files_total: u64,
    pub bytes_total: u64,
    pub files_done: u64,
    pub bytes_done: u64,
    pub skipped: u64,
    pub cached: u64,
    pub errors: u64,
    pub findings: u64,
    pub current: String,
    pub elapsed_ms: u64,
    pub engine: String,
    pub threads: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScanSummary {
    pub id: String,
    pub started: u64,
    pub finished: u64,
    pub duration_ms: u64,
    pub targets: Vec<String>,
    /// completed | cancelled
    pub status: String,
    pub engine: String,
    pub signature_version: String,
    pub files_scanned: u64,
    pub bytes_scanned: u64,
    pub skipped: u64,
    pub cached: u64,
    pub errors: u64,
    pub findings: Vec<Finding>,
    pub error_list: Vec<ErrorEntry>,
}

pub struct ScanOptions {
    pub threads: usize,
    pub max_bytes: u64,
    pub follow_symlinks: bool,
    pub skip_hidden: bool,
    pub heuristics: bool,
    pub skip_unchanged: bool,
    pub exclusions: Vec<PathBuf>,
    pub excluded_ext: Vec<String>,
    pub whitelist: HashSet<String>,
    pub socket: Option<PathBuf>,
    pub signature_version: String,
}

impl ScanOptions {
    fn is_excluded(&self, p: &Path) -> bool {
        self.exclusions.iter().any(|x| p.starts_with(x))
    }
    fn ext_excluded(&self, p: &Path) -> bool {
        if self.excluded_ext.is_empty() {
            return false;
        }
        match p.extension() {
            Some(e) => {
                let e = e.to_string_lossy().to_lowercase();
                self.excluded_ext.iter().any(|x| *x == e)
            }
            None => false,
        }
    }
}

struct Job {
    path: PathBuf,
    size: u64,
    mtime: i64,
    ino: u64,
}

#[derive(Default)]
struct Counters {
    files_total: AtomicU64,
    bytes_total: AtomicU64,
    files_done: AtomicU64,
    bytes_done: AtomicU64,
    skipped: AtomicU64,
    cached: AtomicU64,
    errors: AtomicU64,
    findings: AtomicU64,
    connect_failures: AtomicU64,
    walk_done: AtomicBool,
}

// ---------------------------------------------------------------- cache

/// Remembers files ClamAV found clean with a given signature version, so a
/// re-scan skips unchanged files (same size, mtime and inode).
struct ScanCache {
    enabled: bool,
    sig: String,
    map: Mutex<HashMap<String, (u64, i64, u64)>>,
    dirty: AtomicBool,
}

impl ScanCache {
    fn file() -> PathBuf {
        paths::data_dir().join("scan-cache.tsv")
    }

    fn load(sig: &str, enabled: bool) -> Self {
        let mut map = HashMap::new();
        if enabled && !sig.is_empty() {
            if let Ok(s) = fs::read_to_string(Self::file()) {
                let mut lines = s.lines();
                if lines.next() == Some(&format!("#sig\t{sig}")) {
                    for l in lines {
                        let mut it = l.splitn(4, '\t');
                        if let (Some(a), Some(b), Some(c), Some(p)) = (it.next(), it.next(), it.next(), it.next()) {
                            if let (Ok(a), Ok(b), Ok(c)) = (a.parse(), b.parse(), c.parse()) {
                                map.insert(p.to_string(), (a, b, c));
                            }
                        }
                    }
                }
            }
        }
        ScanCache { enabled: enabled && !sig.is_empty(), sig: sig.to_string(), map: Mutex::new(map), dirty: AtomicBool::new(false) }
    }

    fn key(p: &Path) -> Option<&str> {
        let s = p.to_str()?;
        if s.contains('\n') || s.contains('\t') {
            None
        } else {
            Some(s)
        }
    }

    fn is_clean(&self, j: &Job) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(k) = Self::key(&j.path) else { return false };
        self.map.lock().unwrap().get(k) == Some(&(j.size, j.mtime, j.ino))
    }

    fn mark_clean(&self, j: &Job) {
        if !self.enabled {
            return;
        }
        if let Some(k) = Self::key(&j.path) {
            let mut m = self.map.lock().unwrap();
            if m.len() < 3_000_000 {
                m.insert(k.to_string(), (j.size, j.mtime, j.ino));
                self.dirty.store(true, Relaxed);
            }
        }
    }

    fn save(&self) {
        if !self.enabled || !self.dirty.load(Relaxed) {
            return;
        }
        let m = self.map.lock().unwrap();
        let mut out = String::with_capacity(m.len() * 80);
        out.push_str(&format!("#sig\t{}\n", self.sig));
        for (k, (a, b, c)) in m.iter() {
            out.push_str(&format!("{a}\t{b}\t{c}\t{k}\n"));
        }
        let tmp = Self::file().with_extension("tmp");
        if fs::write(&tmp, out).is_ok() {
            let _ = fs::rename(tmp, Self::file());
        }
    }
}

pub fn clear_cache() {
    let _ = fs::remove_file(ScanCache::file());
}

// ---------------------------------------------------------------- clamd

enum ClamResult {
    Clean,
    Found(String),
    Error(String),
}

fn clam_scan(sock: &Path, path: &Path) -> ClamResult {
    let mut s = match UnixStream::connect(sock) {
        Ok(s) => s,
        Err(e) => return ClamResult::Error(format!("connect: {e}")),
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(1800)));
    let mut cmd = b"zSCAN ".to_vec();
    cmd.extend_from_slice(path.as_os_str().as_bytes());
    cmd.push(0);
    if let Err(e) = s.write_all(&cmd) {
        return ClamResult::Error(e.to_string());
    }
    let mut buf = Vec::new();
    if let Err(e) = s.read_to_end(&mut buf) {
        return ClamResult::Error(e.to_string());
    }
    let resp = String::from_utf8_lossy(&buf);
    let resp = resp.trim_end_matches(&['\0', '\n'][..]);
    if let Some(body) = resp.strip_suffix(" FOUND") {
        let sig = body.rsplit(": ").next().unwrap_or("Unknown").to_string();
        ClamResult::Found(sig)
    } else if resp.ends_with(": OK") {
        ClamResult::Clean
    } else {
        let msg = resp.rsplit(": ").next().unwrap_or(resp).trim_end_matches(" ERROR").to_string();
        ClamResult::Error(msg)
    }
}

// ---------------------------------------------------------------- walk

fn walk(
    targets: &[PathBuf],
    o: &ScanOptions,
    ctl: &ScanControl,
    mut on_file: impl FnMut(Job) -> bool,
    mut on_err: impl FnMut(String, String),
) {
    for t in targets {
        let it = WalkDir::new(t)
            .follow_links(o.follow_symlinks)
            .into_iter()
            .filter_entry(|e| {
                if e.depth() == 0 {
                    return true;
                }
                if o.skip_hidden && e.file_name().as_bytes().first() == Some(&b'.') {
                    return false;
                }
                !o.is_excluded(e.path())
            });
        for entry in it {
            if ctl.cancel.load(Relaxed) {
                return;
            }
            let e = match entry {
                Ok(e) => e,
                Err(err) => {
                    let p = err.path().map(|p| p.display().to_string()).unwrap_or_default();
                    let m = err.io_error().map(|x| x.to_string()).unwrap_or_else(|| err.to_string());
                    on_err(p, m);
                    continue;
                }
            };
            if !e.file_type().is_file() {
                continue;
            }
            if o.ext_excluded(e.path()) {
                continue;
            }
            let md = match e.metadata() {
                Ok(m) => m,
                Err(err) => {
                    on_err(e.path().display().to_string(), err.to_string());
                    continue;
                }
            };
            let path = if e.path_is_symlink() {
                fs::canonicalize(e.path()).unwrap_or_else(|_| e.path().to_path_buf())
            } else {
                e.path().to_path_buf()
            };
            if !on_file(Job { path, size: md.len(), mtime: md.mtime(), ino: md.ino() }) {
                return;
            }
        }
    }
}

// ---------------------------------------------------------------- run

pub fn run(
    app: AppHandle,
    id: String,
    targets: Vec<String>,
    o: ScanOptions,
    ctl: Arc<ScanControl>,
    store: Arc<Mutex<Progress>>,
) -> ScanSummary {
    let started = util::now_ms();
    let t0 = Instant::now();
    let tpaths: Arc<Vec<PathBuf>> = Arc::new(targets.iter().map(PathBuf::from).collect());
    let engine_name = if o.socket.is_some() { "clamav" } else { "lakuen" }.to_string();
    let threads = o.threads.max(1);
    let cache = Arc::new(ScanCache::load(&o.signature_version, o.skip_unchanged && o.socket.is_some()));
    let o = Arc::new(o);
    let c = Arc::new(Counters::default());
    let findings: Arc<Mutex<Vec<Finding>>> = Arc::new(Mutex::new(Vec::new()));
    let errors: Arc<Mutex<Vec<ErrorEntry>>> = Arc::new(Mutex::new(Vec::new()));
    let current = Arc::new(Mutex::new(String::new()));
    let done = Arc::new(AtomicBool::new(false));

    // Counter: walks ahead to know the total for progress and ETA.
    let counter = {
        let (t, o, ctl, c) = (tpaths.clone(), o.clone(), ctl.clone(), c.clone());
        std::thread::spawn(move || {
            walk(
                &t,
                &o,
                &ctl,
                |j| {
                    c.files_total.fetch_add(1, Relaxed);
                    c.bytes_total.fetch_add(j.size, Relaxed);
                    true
                },
                |_, _| {},
            );
            c.walk_done.store(true, Relaxed);
        })
    };

    let (tx, rx) = bounded::<Job>(8192);
    let producer = {
        let (t, o, ctl, c, errors) = (tpaths.clone(), o.clone(), ctl.clone(), c.clone(), errors.clone());
        std::thread::spawn(move || {
            walk(
                &t,
                &o,
                &ctl,
                |j| tx.send(j).is_ok(),
                |p, m| {
                    c.errors.fetch_add(1, Relaxed);
                    let mut e = errors.lock().unwrap();
                    if e.len() < 2000 {
                        e.push(ErrorEntry { path: p, message: m });
                    }
                },
            );
        })
    };

    let mut workers = Vec::new();
    for _ in 0..threads {
        let (rx, o, c, ctl, cache, findings, errors, current, app) = (
            rx.clone(),
            o.clone(),
            c.clone(),
            ctl.clone(),
            cache.clone(),
            findings.clone(),
            errors.clone(),
            current.clone(),
            app.clone(),
        );
        workers.push(std::thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                if ctl.cancel.load(Relaxed) {
                    break;
                }
                while ctl.paused.load(Relaxed) && !ctl.cancel.load(Relaxed) {
                    std::thread::sleep(Duration::from_millis(150));
                }
                if ctl.cancel.load(Relaxed) {
                    break;
                }
                let size = job.size;
                let finish = |c: &Counters| {
                    c.files_done.fetch_add(1, Relaxed);
                    c.bytes_done.fetch_add(size, Relaxed);
                };
                if size > o.max_bytes {
                    c.skipped.fetch_add(1, Relaxed);
                    finish(&c);
                    continue;
                }
                if cache.is_clean(&job) {
                    c.cached.fetch_add(1, Relaxed);
                    finish(&c);
                    continue;
                }
                if let Ok(mut cur) = current.try_lock() {
                    *cur = job.path.display().to_string();
                }

                let mut hit: Option<(String, String, String)> = None;
                let mut clam_clean = false;
                let use_clam = o.socket.is_some() && c.connect_failures.load(Relaxed) < 50;
                if use_clam {
                    match clam_scan(o.socket.as_ref().unwrap(), &job.path) {
                        ClamResult::Clean => clam_clean = true,
                        ClamResult::Found(sig) => {
                            let sev = if sig.starts_with("PUA.") { "pua" } else { "infected" };
                            hit = Some((sig, sev.into(), "clamav".into()));
                        }
                        ClamResult::Error(m) => {
                            if m.starts_with("connect:") {
                                c.connect_failures.fetch_add(1, Relaxed);
                            }
                            c.errors.fetch_add(1, Relaxed);
                            let mut e = errors.lock().unwrap();
                            if e.len() < 2000 {
                                e.push(ErrorEntry { path: job.path.display().to_string(), message: m });
                            }
                        }
                    }
                }
                if hit.is_none() && o.heuristics {
                    if let Some(h) = heuristics::check(&job.path, size) {
                        hit = Some((h.name, h.severity.to_string(), "lakuen".into()));
                    }
                }
                match hit {
                    Some((sig, sev, eng)) => {
                        let ignored = !o.whitelist.is_empty()
                            && util::sha256_file(&job.path).map(|h| o.whitelist.contains(&h)).unwrap_or(false);
                        if !ignored {
                            let f = Finding {
                                id: util::new_id(),
                                path: job.path.display().to_string(),
                                signature: sig,
                                severity: sev,
                                engine: eng,
                                size,
                                status: "open".into(),
                            };
                            c.findings.fetch_add(1, Relaxed);
                            findings.lock().unwrap().push(f.clone());
                            let _ = app.emit("scan-finding", f);
                        }
                    }
                    None => {
                        if clam_clean {
                            cache.mark_clean(&job);
                        }
                    }
                }
                finish(&c);
            }
        }));
    }
    drop(rx);

    let snapshot = {
        let (c, ctl, current, id, engine_name) = (c.clone(), ctl.clone(), current.clone(), id.clone(), engine_name.clone());
        move |running: bool| Progress {
            scan_id: id.clone(),
            running,
            paused: ctl.paused.load(Relaxed),
            cancelled: ctl.cancel.load(Relaxed),
            walk_done: c.walk_done.load(Relaxed),
            files_total: c.files_total.load(Relaxed),
            bytes_total: c.bytes_total.load(Relaxed),
            files_done: c.files_done.load(Relaxed),
            bytes_done: c.bytes_done.load(Relaxed),
            skipped: c.skipped.load(Relaxed),
            cached: c.cached.load(Relaxed),
            errors: c.errors.load(Relaxed),
            findings: c.findings.load(Relaxed),
            current: current.lock().map(|s| s.clone()).unwrap_or_default(),
            elapsed_ms: t0.elapsed().as_millis() as u64,
            engine: engine_name.clone(),
            threads,
        }
    };
    let snapshot = Arc::new(snapshot);

    let emitter = {
        let (done, store, app, snap) = (done.clone(), store.clone(), app.clone(), snapshot.clone());
        std::thread::spawn(move || {
            while !done.load(Relaxed) {
                let p = snap(true);
                *store.lock().unwrap() = p.clone();
                let _ = app.emit("scan-progress", p);
                std::thread::sleep(Duration::from_millis(200));
            }
        })
    };

    let _ = producer.join();
    for w in workers {
        let _ = w.join();
    }
    let _ = counter.join();
    done.store(true, Relaxed);
    let _ = emitter.join();
    cache.save();

    let mut fin = snapshot(false);
    fin.walk_done = true;
    if !fin.cancelled {
        fin.files_total = fin.files_done;
        fin.bytes_total = fin.bytes_done;
    }
    *store.lock().unwrap() = fin.clone();
    let _ = app.emit("scan-progress", fin.clone());

    let findings = findings.lock().unwrap().clone();
    let error_list = errors.lock().unwrap().clone();
    ScanSummary {
        id,
        started,
        finished: util::now_ms(),
        duration_ms: t0.elapsed().as_millis() as u64,
        targets,
        status: if fin.cancelled { "cancelled".into() } else { "completed".into() },
        engine: engine_name,
        signature_version: o.signature_version.clone(),
        files_scanned: fin.files_done,
        bytes_scanned: fin.bytes_done,
        skipped: fin.skipped,
        cached: fin.cached,
        errors: fin.errors,
        findings,
        error_list,
    }
}

pub fn quick_targets() -> Vec<String> {
    let mut v = Vec::new();
    let home = dirs::home_dir();
    let mut add = |p: PathBuf| {
        if p.exists() {
            v.push(p.display().to_string());
        }
    };
    if let Some(h) = &home {
        for d in [
            "Downloads",
            "Desktop",
            "Schreibtisch",
            "Applications",
            ".local/bin",
            ".local/share/applications",
            ".config/autostart",
        ] {
            add(h.join(d));
        }
    }
    if let Some(d) = dirs::download_dir() {
        if !v.contains(&d.display().to_string()) {
            add(d);
        }
    }
    for d in ["/tmp", "/var/tmp", "/dev/shm"] {
        add(PathBuf::from(d));
    }
    v.dedup();
    v
}

pub fn engine_socket_if_ready(ready: bool) -> Option<PathBuf> {
    if ready {
        Some(engine::socket_path())
    } else {
        None
    }
}
