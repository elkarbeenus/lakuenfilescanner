use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn new_id() -> String {
    format!("{:x}{:04x}", now_ms(), COUNTER.fetch_add(1, Ordering::Relaxed) & 0xffff)
}

pub fn sha256_file(p: &Path) -> std::io::Result<String> {
    let mut f = File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Remove variables the AppImage runtime sets for our own process so that
/// external programs (xdg-open, notify-send, clamd) see a clean environment.
pub fn clean_env(c: &mut Command) {
    for k in [
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "GTK_PATH",
        "GTK_EXE_PREFIX",
        "GTK_DATA_PREFIX",
        "GTK_IM_MODULE_FILE",
        "GDK_PIXBUF_MODULE_FILE",
        "GDK_PIXBUF_MODULEDIR",
        "GIO_MODULE_DIR",
        "GIO_EXTRA_MODULES",
        "GSETTINGS_SCHEMA_DIR",
        "PYTHONHOME",
        "PYTHONPATH",
        "PERLLIB",
        "QT_PLUGIN_PATH",
        "GST_PLUGIN_SYSTEM_PATH",
        "GST_PLUGIN_SYSTEM_PATH_1_0",
        "GST_PLUGIN_SCANNER",
    ] {
        c.env_remove(k);
    }
}

pub fn expand(s: &str) -> PathBuf {
    let home = dirs::home_dir();
    if s == "~" {
        return home.unwrap_or_else(|| PathBuf::from(s));
    }
    if let Some(r) = s.strip_prefix("~/") {
        if let Some(h) = home {
            return h.join(r);
        }
    }
    PathBuf::from(s)
}

fn spawn_detached(mut c: Command) -> Result<(), String> {
    clean_env(&mut c);
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = c.spawn().map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn open_path(p: &Path) -> Result<(), String> {
    let mut c = Command::new("xdg-open");
    c.arg(p);
    spawn_detached(c)
}

/// Open the folder containing `p` (or `p` itself if it is a folder).
pub fn reveal(p: &Path) -> Result<(), String> {
    let target = if p.is_dir() {
        p.to_path_buf()
    } else {
        p.parent().map(|x| x.to_path_buf()).unwrap_or_else(|| PathBuf::from("/"))
    };
    open_path(&target)
}

pub fn notify(title: &str, body: &str) {
    let mut c = Command::new("notify-send");
    c.args(["-a", "LakuenFileScanner", "-i", "security-high", title, body]);
    let _ = spawn_detached(c);
}

pub fn version_label() -> String {
    match option_env!("LAKUEN_VERSION_LABEL") {
        Some(l) if !l.is_empty() => l.to_string(),
        _ => crate::version::readable(env!("CARGO_PKG_VERSION")),
    }
}

pub fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> Option<T> {
    let s = fs::read_to_string(p).ok()?;
    serde_json::from_str(&s).ok()
}

/// Atomic JSON write (temp file + rename).
pub fn write_json<T: serde::Serialize>(p: &Path, v: &T) -> Result<(), String> {
    let tmp = p.with_extension("tmp");
    let s = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    fs::write(&tmp, s).map_err(|e| e.to_string())?;
    fs::rename(&tmp, p).map_err(|e| e.to_string())
}
