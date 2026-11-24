//! ClamAV engine management: a private clamd daemon (signatures loaded once,
//! multi-threaded scanning over a Unix socket) and freshclam for updates.

use crate::{paths, settings::Settings, util};
use serde::Serialize;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, Default, Debug)]
pub struct EngineStatus {
    /// missing | no_db | stopped | starting | ready | error
    pub state: String,
    pub message: String,
    pub engine_version: String,
    pub signature_version: String,
    pub signature_date: String,
    pub db_present: bool,
    pub db_size: u64,
    pub bundled: bool,
    pub available: bool,
    pub updating: bool,
    pub threads: usize,
}

pub struct ClamPaths {
    pub clamd: PathBuf,
    pub freshclam: PathBuf,
    pub lib: Option<PathBuf>,
    pub certs: Option<PathBuf>,
    pub bundled: bool,
}

pub fn locate() -> Option<ClamPaths> {
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Ok(d) = std::env::var("LAKUEN_CLAMAV_DIR") {
        bases.push(PathBuf::from(d));
    }
    if let Ok(d) = std::env::var("APPDIR") {
        bases.push(PathBuf::from(d).join("usr/lib/lakuen-clamav"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            bases.push(p.join("../lib/lakuen-clamav"));
        }
    }
    for b in bases {
        let clamd = b.join("bin/clamd");
        let freshclam = b.join("bin/freshclam");
        if clamd.is_file() && freshclam.is_file() {
            let lib = b.join("lib");
            let certs = b.join("certs");
            return Some(ClamPaths {
                clamd,
                freshclam,
                lib: if lib.is_dir() { Some(lib) } else { None },
                certs: if certs.is_dir() { Some(certs) } else { None },
                bundled: true,
            });
        }
    }
    let find = |c: &[&str]| c.iter().map(PathBuf::from).find(|p| p.is_file());
    let clamd = find(&["/usr/sbin/clamd", "/usr/bin/clamd", "/usr/local/sbin/clamd", "/usr/local/bin/clamd"])?;
    let freshclam = find(&["/usr/bin/freshclam", "/usr/local/bin/freshclam", "/usr/sbin/freshclam"])?;
    Some(ClamPaths { clamd, freshclam, lib: None, certs: None, bundled: false })
}

fn command(cp: &ClamPaths, bin: &Path) -> Command {
    let mut c = Command::new(bin);
    util::clean_env(&mut c);
    if let Some(lib) = &cp.lib {
        c.env("LD_LIBRARY_PATH", lib);
    }
    for ca in [
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/tls/certs/ca-bundle.crt",
        "/etc/ssl/cert.pem",
    ] {
        if Path::new(ca).is_file() {
            c.env("CURL_CA_BUNDLE", ca);
            c.env("SSL_CERT_FILE", ca);
            break;
        }
    }
    c
}

pub fn db_info() -> (bool, u64) {
    let mut present = false;
    let mut size = 0;
    if let Ok(rd) = fs::read_dir(paths::db_dir()) {
        for e in rd.flatten() {
            let p = e.path();
            let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext == "cvd" || ext == "cld" {
                present = true;
                size += e.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    (present, size)
}

pub fn socket_path() -> PathBuf {
    paths::runtime_dir().join("clamd.sock")
}

/// Send one z-command to clamd and return the reply.
pub fn clamd_cmd(sock: &Path, cmd: &str, timeout: Duration) -> Result<String, String> {
    let mut s = UnixStream::connect(sock).map_err(|e| format!("connect: {e}"))?;
    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(timeout));
    s.write_all(format!("z{cmd}\0").as_bytes()).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&buf).trim_end_matches(&['\0', '\n'][..]).to_string())
}

fn parse_version(v: &str) -> (String, String, String) {
    let mut it = v.splitn(3, '/');
    let engine = it.next().unwrap_or("").to_string();
    let sig = it.next().unwrap_or("").to_string();
    let date = it.next().unwrap_or("").to_string();
    (engine, sig, date)
}

fn log_tail(p: &Path, n: usize) -> String {
    let s = fs::read_to_string(p).unwrap_or_default();
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

pub struct Engine {
    child: Mutex<Option<Child>>,
    status: Mutex<EngineStatus>,
    updating: AtomicBool,
    starting: AtomicBool,
}

impl Engine {
    pub fn new() -> Self {
        let cp = locate();
        let (db_present, db_size) = db_info();
        let state = if cp.is_none() {
            "missing"
        } else if !db_present {
            "no_db"
        } else {
            "stopped"
        };
        Engine {
            child: Mutex::new(None),
            status: Mutex::new(EngineStatus {
                state: state.into(),
                available: cp.is_some(),
                bundled: cp.as_ref().map(|c| c.bundled).unwrap_or(false),
                db_present,
                db_size,
                ..Default::default()
            }),
            updating: AtomicBool::new(false),
            starting: AtomicBool::new(false),
        }
    }

    pub fn status(&self) -> EngineStatus {
        let (db_present, db_size) = db_info();
        let mut s = self.status.lock().unwrap();
        s.db_present = db_present;
        s.db_size = db_size;
        s.updating = self.updating.load(Ordering::SeqCst);
        s.clone()
    }

    fn set(&self, app: &AppHandle, f: impl FnOnce(&mut EngineStatus)) {
        let snap = {
            let mut g = self.status.lock().unwrap();
            f(&mut g);
            let (p, sz) = db_info();
            g.db_present = p;
            g.db_size = sz;
            g.updating = self.updating.load(Ordering::SeqCst);
            g.clone()
        };
        let _ = app.emit("engine-status", snap);
    }

    pub fn is_ready(&self) -> bool {
        self.status.lock().unwrap().state == "ready"
    }

    pub fn signature_version(&self) -> String {
        self.status.lock().unwrap().signature_version.clone()
    }

    pub fn start(&self, app: &AppHandle, settings: &Settings) -> Result<(), String> {
        if self.starting.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let r = self.start_inner(app, settings);
        self.starting.store(false, Ordering::SeqCst);
        if let Err(e) = &r {
            eprintln!("engine start failed: {e}");
        }
        r
    }

    fn start_inner(&self, app: &AppHandle, settings: &Settings) -> Result<(), String> {
        let Some(cp) = locate() else {
            self.set(app, |s| {
                s.state = "missing".into();
                s.message = "ClamAV engine not found".into();
            });
            return Err("ClamAV engine not found".into());
        };
        if !db_info().0 {
            self.set(app, |s| {
                s.state = "no_db".into();
                s.message = String::new();
            });
            return Err("no signatures".into());
        }
        self.stop();
        kill_stale();

        let threads = settings.effective_threads();
        let run = paths::runtime_dir();
        let tmp = run.join("tmp");
        let _ = fs::create_dir_all(&tmp);
        let sock = socket_path();
        let _ = fs::remove_file(&sock);
        let max_file = settings.max_file_mb.clamp(25, 4000);
        let max_scan = (max_file * 2).min(4000);
        let mut conf = format!(
            "LocalSocket {sock}\nLocalSocketMode 600\nFixStaleSocket yes\nForeground yes\n\
             DatabaseDirectory {db}\nTemporaryDirectory {tmp}\nPidFile {pid}\n\
             LogFile {log}\nLogFileMaxSize 5M\nLogTime yes\n\
             MaxThreads {threads}\nMaxQueue {queue}\nMaxConnectionQueueLength 256\n\
             ReadTimeout 600\nCommandReadTimeout 30\nIdleTimeout 60\nSelfCheck 3600\n\
             ConcurrentDatabaseReload no\nExitOnOOM no\n\
             MaxFileSize {max_file}M\nMaxScanSize {max_scan}M\nMaxRecursion 17\nMaxFiles 15000\n\
             ScanArchive {arch}\nDetectPUA {pua}\nAlertExceedsMax no\nBytecode yes\n",
            sock = sock.display(),
            db = paths::db_dir().display(),
            tmp = tmp.display(),
            pid = run.join("clamd.pid").display(),
            log = run.join("clamd.log").display(),
            threads = threads,
            queue = (threads * 8).max(100),
            arch = if settings.scan_archives { "yes" } else { "no" },
            pua = if settings.detect_pua { "yes" } else { "no" },
        );
        if let Some(c) = &cp.certs {
            conf.push_str(&format!("CVDCertsDirectory {}\n", c.display()));
        }
        let conf_path = run.join("clamd.conf");
        fs::write(&conf_path, conf).map_err(|e| e.to_string())?;
        let _ = fs::remove_file(run.join("clamd.log"));

        self.set(app, |s| {
            s.state = "starting".into();
            s.message = String::new();
            s.threads = threads;
        });

        let stderr_path = run.join("clamd.stderr.log");
        let stderr = File::create(&stderr_path).map_err(|e| e.to_string())?;
        let mut cmd = command(&cp, &cp.clamd);
        cmd.arg("--foreground")
            .arg(format!("--config-file={}", conf_path.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr));
        let child = cmd.spawn().map_err(|e| {
            let m = format!("clamd: {e}");
            self.set(app, |s| {
                s.state = "error".into();
                s.message = m.clone();
            });
            m
        })?;
        let _ = fs::write(run.join("lakuen-clamd.pid"), child.id().to_string());
        *self.child.lock().unwrap() = Some(child);

        let t0 = Instant::now();
        loop {
            let exited = {
                let mut g = self.child.lock().unwrap();
                match g.as_mut() {
                    Some(c) => c.try_wait().ok().flatten().is_some(),
                    None => true,
                }
            };
            if exited {
                *self.child.lock().unwrap() = None;
                let mut msg = log_tail(&stderr_path, 6);
                if msg.is_empty() {
                    msg = log_tail(&run.join("clamd.log"), 6);
                }
                self.set(app, |s| {
                    s.state = "error".into();
                    s.message = msg.clone();
                });
                return Err(msg);
            }
            if clamd_cmd(&sock, "PING", Duration::from_secs(2)).map(|r| r == "PONG").unwrap_or(false) {
                break;
            }
            if t0.elapsed() > Duration::from_secs(600) {
                self.stop();
                self.set(app, |s| {
                    s.state = "error".into();
                    s.message = "clamd start timeout".into();
                });
                return Err("timeout".into());
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        self.refresh_version(app);
        Ok(())
    }

    pub fn refresh_version(&self, app: &AppHandle) {
        let v = clamd_cmd(&socket_path(), "VERSION", Duration::from_secs(5)).unwrap_or_default();
        let (e, sig, date) = parse_version(&v);
        self.set(app, |s| {
            s.state = "ready".into();
            s.message = String::new();
            s.engine_version = e;
            s.signature_version = sig;
            s.signature_date = date;
        });
    }

    pub fn stop(&self) {
        if let Some(mut c) = self.child.lock().unwrap().take() {
            unsafe {
                libc::kill(c.id() as i32, libc::SIGTERM);
            }
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(3) {
                if c.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = fs::remove_file(paths::runtime_dir().join("lakuen-clamd.pid"));
        let mut s = self.status.lock().unwrap();
        if s.state == "ready" || s.state == "starting" {
            s.state = "stopped".into();
        }
    }

    pub fn update_signatures(&self, app: &AppHandle, settings: &Settings) -> Result<(), String> {
        if self.updating.swap(true, Ordering::SeqCst) {
            return Err("busy".into());
        }
        self.set(app, |_| {});
        let res = self.run_freshclam(app);
        self.updating.store(false, Ordering::SeqCst);
        match &res {
            Ok(()) => {
                if self.is_ready() {
                    let old = self.signature_version();
                    let _ = clamd_cmd(&socket_path(), "RELOAD", Duration::from_secs(5));
                    self.set(app, |s| s.state = "starting".into());
                    let t0 = Instant::now();
                    while t0.elapsed() < Duration::from_secs(180) {
                        std::thread::sleep(Duration::from_millis(1000));
                        let v = clamd_cmd(&socket_path(), "VERSION", Duration::from_secs(60)).unwrap_or_default();
                        let (_, sig, _) = parse_version(&v);
                        if !sig.is_empty() && sig != old {
                            break;
                        }
                        // up to date already: version stays the same
                        if t0.elapsed() > Duration::from_secs(15) && !sig.is_empty() {
                            break;
                        }
                    }
                    self.refresh_version(app);
                } else {
                    let _ = self.start(app, settings);
                }
            }
            Err(e) => {
                let e = e.clone();
                self.set(app, |s| s.message = e);
            }
        }
        res
    }

    fn run_freshclam(&self, app: &AppHandle) -> Result<(), String> {
        let cp = locate().ok_or("ClamAV engine not found")?;
        let run = paths::runtime_dir();
        let mut conf = format!(
            "DatabaseDirectory {}\nDatabaseMirror database.clamav.net\nDNSDatabaseInfo current.cvd.clamav.net\n\
             ConnectTimeout 30\nReceiveTimeout 0\nTestDatabases no\nBytecode yes\nLogTime no\n",
            paths::db_dir().display()
        );
        if let Some(c) = &cp.certs {
            conf.push_str(&format!("CVDCertsDirectory {}\n", c.display()));
        }
        let conf_path = run.join("freshclam.conf");
        fs::write(&conf_path, conf).map_err(|e| e.to_string())?;

        let mut cmd = command(&cp, &cp.freshclam);
        cmd.arg(format!("--config-file={}", conf_path.display()))
            .arg("--stdout")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let _ = app.emit("freshclam-log", "▶ freshclam");
        let mut child = cmd.spawn().map_err(|e| format!("freshclam: {e}"))?;
        let out = child.stdout.take();
        let err = child.stderr.take();
        let a1 = app.clone();
        let t_err = std::thread::spawn(move || {
            if let Some(e) = err {
                pump(e, &a1);
            }
        });
        if let Some(o) = out {
            pump(o, app);
        }
        let _ = t_err.join();
        let status = child.wait().map_err(|e| e.to_string())?;
        let code = status.code().unwrap_or(-1);
        let _ = app.emit("freshclam-log", format!("■ exit {code}"));
        if code == 0 || code == 1 {
            Ok(())
        } else {
            Err(format!("freshclam exit code {code}"))
        }
    }
}

fn pump<R: Read>(r: R, app: &AppHandle) {
    let reader = BufReader::new(r);
    for chunk in reader.split(b'\n').flatten() {
        let s = String::from_utf8_lossy(&chunk).to_string();
        for part in s.split('\r') {
            let p = part.trim();
            if !p.is_empty() {
                let _ = app.emit("freshclam-log", p.to_string());
            }
        }
    }
}

/// Kill a clamd left over from a crashed session of this app.
fn kill_stale() {
    let pidf = paths::runtime_dir().join("lakuen-clamd.pid");
    if let Ok(s) = fs::read_to_string(&pidf) {
        if let Ok(pid) = s.trim().parse::<i32>() {
            let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            if String::from_utf8_lossy(&cmdline).contains("lakuenfilescanner") {
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
                std::thread::sleep(Duration::from_millis(800));
            }
        }
        let _ = fs::remove_file(pidf);
    }
}
