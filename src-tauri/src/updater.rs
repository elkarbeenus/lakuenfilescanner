//! In-app updater: checks this repo's GitHub releases, downloads the new
//! AppImage, verifies SHA-256, replaces the running AppImage atomically and
//! restarts. Settings, quarantine and signatures live outside the AppImage
//! and are kept.

use crate::{util, version};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

pub const REPO: &str = "elkarbeenus/lakuenfilescanner";
const UA: &str = concat!("LakuenFileScanner/", env!("CARGO_PKG_VERSION"));

#[derive(Serialize, Clone, Default, Debug)]
pub struct UpdateInfo {
    pub available: bool,
    pub current: String,
    pub current_label: String,
    pub latest: String,
    pub latest_label: String,
    pub notes: String,
    pub page_url: String,
    pub asset_url: String,
    pub sha_url: String,
    pub size: u64,
    pub can_self_update: bool,
    pub test_build: bool,
}

#[derive(Serialize, Clone)]
struct DlProgress {
    downloaded: u64,
    total: u64,
    stage: String,
}

pub fn check() -> Result<UpdateInfo, String> {
    let mut info = UpdateInfo {
        current: env!("CARGO_PKG_VERSION").to_string(),
        current_label: util::version_label(),
        can_self_update: std::env::var_os("APPIMAGE").is_some(),
        ..Default::default()
    };
    if std::env::var_os("LAKUEN_NO_UPDATE_CHECK").is_some() {
        return Ok(info);
    }
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let v: serde_json::Value = ureq::get(&url)
        .set("User-Agent", UA)
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(15))
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().unwrap_or("").to_string();
    info.latest = tag.clone();
    info.latest_label = version::readable(&tag);
    info.notes = v["body"].as_str().unwrap_or("").to_string();
    info.page_url = v["html_url"].as_str().unwrap_or("").to_string();
    info.test_build = version::parse(&tag).map(|x| x.test.is_some()).unwrap_or(false);
    if let Some(assets) = v["assets"].as_array() {
        for a in assets {
            let name = a["name"].as_str().unwrap_or("");
            let dl = a["browser_download_url"].as_str().unwrap_or("").to_string();
            if name.ends_with(".AppImage") && name.contains("x86_64") {
                info.asset_url = dl;
                info.size = a["size"].as_u64().unwrap_or(0);
            } else if name.ends_with(".AppImage.sha256") {
                info.sha_url = dl;
            }
        }
    }
    info.available = !info.asset_url.is_empty() && version::is_newer(&tag, &info.current);
    Ok(info)
}

pub fn apply(app: &AppHandle) -> Result<(), String> {
    let info = check()?;
    if !info.available {
        return Err("no update".into());
    }
    let target = PathBuf::from(std::env::var("APPIMAGE").map_err(|_| "not running as AppImage")?);
    let tmp = PathBuf::from(format!("{}.download", target.display()));

    let emit = |d: u64, t: u64, s: &str| {
        let _ = app.emit("update-progress", DlProgress { downloaded: d, total: t, stage: s.into() });
    };

    let expected = if info.sha_url.is_empty() {
        None
    } else {
        let s = ureq::get(&info.sha_url)
            .set("User-Agent", UA)
            .timeout(Duration::from_secs(30))
            .call()
            .map_err(|e| e.to_string())?
            .into_string()
            .map_err(|e| e.to_string())?;
        s.split_whitespace().next().map(|x| x.to_lowercase())
    };

    let resp = ureq::get(&info.asset_url)
        .set("User-Agent", UA)
        .call()
        .map_err(|e| e.to_string())?;
    let total: u64 = resp.header("Content-Length").and_then(|s| s.parse().ok()).unwrap_or(info.size);
    let mut reader = resp.into_reader();
    let mut out = File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done: u64 = 0;
    let mut last = Instant::now();
    loop {
        let n = reader.read(&mut buf).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            e.to_string()
        })?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        h.update(&buf[..n]);
        done += n as u64;
        if last.elapsed() > Duration::from_millis(150) {
            emit(done, total, "download");
            last = Instant::now();
        }
    }
    out.sync_all().map_err(|e| e.to_string())?;
    drop(out);
    emit(done, total, "verify");
    let got = hex::encode(h.finalize());
    if let Some(exp) = expected {
        if exp != got {
            let _ = fs::remove_file(&tmp);
            return Err("checksum mismatch".into());
        }
    }
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
    emit(done, total, "restart");

    let mut c = Command::new(&target);
    util::clean_env(&mut c);
    for k in ["APPDIR", "APPIMAGE", "ARGV0", "OWD"] {
        c.env_remove(k);
    }
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    c.spawn().map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(300));
    app.exit(0);
    Ok(())
}
