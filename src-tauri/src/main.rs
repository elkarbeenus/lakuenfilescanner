// LakuenFileScanner – erstellt von Dominic Lesina x Claude
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;
mod heuristics;
mod history;
mod paths;
mod quarantine;
mod report;
mod scanner;
mod settings;
mod updater;
mod util;
mod version;

use engine::Engine;
use scanner::{Progress, ScanControl, ScanOptions, ScanSummary};
use serde::Deserialize;
use settings::{Settings, WhitelistEntry};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

pub struct AppState {
    settings: Mutex<Settings>,
    engine: Arc<Engine>,
    scan: Mutex<Option<Arc<ScanControl>>>,
    progress: Arc<Mutex<Progress>>,
    last_summary: Mutex<Option<ScanSummary>>,
    startup_targets: Mutex<Vec<String>>,
}

type R<T> = Result<T, String>;

// ------------------------------------------------------------ app / settings

#[tauri::command]
fn app_info() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "label": util::version_label(),
        "repo": updater::REPO,
        "appimage": std::env::var_os("APPIMAGE").is_some(),
        "cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4),
        "data_dir": paths::data_dir().display().to_string(),
    })
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> R<Settings> {
    let old_key = state.settings.lock().unwrap().engine_key();
    settings.save()?;
    *state.settings.lock().unwrap() = settings.clone();
    if old_key != settings.engine_key() && state.engine.is_ready() {
        let eng = state.engine.clone();
        let s = settings.clone();
        std::thread::spawn(move || {
            let _ = eng.start(&app, &s);
        });
    }
    Ok(settings)
}

fn mutate_settings(state: &State<'_, AppState>, f: impl FnOnce(&mut Settings)) -> R<Settings> {
    let mut g = state.settings.lock().unwrap();
    f(&mut g);
    g.save()?;
    Ok(g.clone())
}

#[tauri::command]
fn startup_targets(state: State<'_, AppState>) -> Vec<String> {
    std::mem::take(&mut *state.startup_targets.lock().unwrap())
}

// ------------------------------------------------------------ engine

#[tauri::command]
fn engine_status(state: State<'_, AppState>) -> engine::EngineStatus {
    state.engine.status()
}

#[tauri::command]
fn engine_start(app: AppHandle, state: State<'_, AppState>) {
    let eng = state.engine.clone();
    let s = state.settings.lock().unwrap().clone();
    std::thread::spawn(move || {
        let _ = eng.start(&app, &s);
    });
}

#[tauri::command]
fn update_signatures(app: AppHandle, state: State<'_, AppState>) {
    spawn_signature_update(app, state.engine.clone());
}

fn spawn_signature_update(app: AppHandle, eng: Arc<Engine>) {
    std::thread::spawn(move || {
        let st = app.state::<AppState>();
        let s = st.settings.lock().unwrap().clone();
        let r = eng.update_signatures(&app, &s);
        if r.is_ok() {
            let mut g = st.settings.lock().unwrap();
            g.last_signature_update = util::now_ms();
            let _ = g.save();
        }
        let _ = app.emit("signatures-updated", r.err().unwrap_or_default());
    });
}

// ------------------------------------------------------------ pickers

fn fp_to_string(f: tauri_plugin_dialog::FilePath) -> Option<String> {
    f.into_path().ok().map(|p| p.display().to_string())
}

#[tauri::command]
async fn pick_files(app: AppHandle) -> Vec<String> {
    app.dialog()
        .file()
        .blocking_pick_files()
        .map(|v| v.into_iter().filter_map(fp_to_string).collect())
        .unwrap_or_default()
}

#[tauri::command]
async fn pick_folders(app: AppHandle) -> Vec<String> {
    app.dialog()
        .file()
        .blocking_pick_folders()
        .map(|v| v.into_iter().filter_map(fp_to_string).collect())
        .unwrap_or_default()
}

#[tauri::command]
async fn pick_save(app: AppHandle, name: String, ext: String) -> Option<String> {
    app.dialog()
        .file()
        .set_file_name(&name)
        .add_filter(ext.to_uppercase(), &[ext.as_str()])
        .blocking_save_file()
        .and_then(fp_to_string)
}

// ------------------------------------------------------------ scanning

#[tauri::command]
fn quick_targets() -> Vec<String> {
    scanner::quick_targets()
}

#[tauri::command]
fn start_scan(app: AppHandle, state: State<'_, AppState>, targets: Vec<String>) -> R<String> {
    let mut slot = state.scan.lock().unwrap();
    if slot.is_some() {
        return Err("busy".into());
    }
    let targets: Vec<String> = targets
        .into_iter()
        .map(|t| {
            let p = util::expand(&t);
            std::fs::canonicalize(&p).unwrap_or(p).display().to_string()
        })
        .filter(|t| std::path::Path::new(t).exists())
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Err("no targets".into());
    }
    let s = {
        let mut g = state.settings.lock().unwrap();
        g.push_recent(&targets);
        let _ = g.save();
        g.clone()
    };
    let ready = state.engine.is_ready();
    let opts = ScanOptions {
        threads: s.effective_threads(),
        max_bytes: s.max_file_mb.saturating_mul(1024 * 1024),
        follow_symlinks: s.follow_symlinks,
        skip_hidden: s.skip_hidden,
        heuristics: s.heuristics,
        skip_unchanged: s.skip_unchanged,
        exclusions: s.exclusions.iter().map(|x| util::expand(x)).collect(),
        excluded_ext: s
            .excluded_extensions
            .iter()
            .map(|x| x.trim().trim_start_matches('.').to_lowercase())
            .filter(|x| !x.is_empty())
            .collect(),
        whitelist: s.whitelist.iter().map(|w| w.sha256.clone()).collect::<HashSet<_>>(),
        socket: scanner::engine_socket_if_ready(ready),
        signature_version: if ready { state.engine.signature_version() } else { String::new() },
    };
    let id = util::new_id();
    let ctl = Arc::new(ScanControl::default());
    *slot = Some(ctl.clone());
    drop(slot);
    *state.progress.lock().unwrap() = Progress { scan_id: id.clone(), running: true, ..Default::default() };
    let _ = app.emit("scan-started", serde_json::json!({ "id": id, "targets": targets }));

    let progress = state.progress.clone();
    let notify = s.notify_on_finish;
    let lang = s.language.clone();
    let id2 = id.clone();
    std::thread::spawn(move || {
        let summary = scanner::run(app.clone(), id2, targets, opts, ctl, progress);
        history::add(&summary);
        let st = app.state::<AppState>();
        *st.last_summary.lock().unwrap() = Some(summary.clone());
        *st.scan.lock().unwrap() = None;
        if notify {
            let n = summary.findings.len();
            let (title, body) = if lang == "en" {
                (
                    if n == 0 { "Scan finished – no threats".to_string() } else { format!("Scan finished – {n} finding(s)") },
                    format!("{} files checked", summary.files_scanned),
                )
            } else {
                (
                    if n == 0 { "Scan fertig – keine Bedrohungen".to_string() } else { format!("Scan fertig – {n} Fund(e)") },
                    format!("{} Dateien geprüft", summary.files_scanned),
                )
            };
            util::notify(&title, &body);
        }
        let _ = app.emit("scan-done", summary);
    });
    Ok(id)
}

#[tauri::command]
fn self_test(app: AppHandle, state: State<'_, AppState>) -> R<String> {
    let dir = paths::data_dir().join("selftest");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let f = dir.join("eicar-test.com");
    std::fs::write(&f, heuristics::eicar()).map_err(|e| e.to_string())?;
    start_scan(app, state, vec![f.display().to_string()])
}

#[tauri::command]
fn pause_scan(state: State<'_, AppState>, paused: bool) {
    if let Some(c) = state.scan.lock().unwrap().as_ref() {
        c.paused.store(paused, std::sync::atomic::Ordering::Relaxed);
    }
}

#[tauri::command]
fn cancel_scan(state: State<'_, AppState>) {
    if let Some(c) = state.scan.lock().unwrap().as_ref() {
        c.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        c.paused.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

#[tauri::command]
fn scan_state(state: State<'_, AppState>) -> serde_json::Value {
    serde_json::json!({
        "running": state.scan.lock().unwrap().is_some(),
        "progress": state.progress.lock().unwrap().clone(),
        "last": state.last_summary.lock().unwrap().clone(),
    })
}

#[tauri::command]
fn clear_scan_cache() {
    scanner::clear_cache();
}

// ------------------------------------------------------------ findings

#[derive(Deserialize)]
struct FindingRef {
    scan_id: String,
    id: String,
    path: String,
    signature: String,
    severity: String,
}

#[derive(serde::Serialize)]
struct ActionResult {
    id: String,
    ok: bool,
    error: String,
}

fn update_last(state: &State<'_, AppState>, id: &str, status: &str) {
    if let Some(s) = state.last_summary.lock().unwrap().as_mut() {
        for f in s.findings.iter_mut().filter(|f| f.id == id) {
            f.status = status.into();
        }
    }
}

#[tauri::command]
async fn quarantine_findings(state: State<'_, AppState>, items: Vec<FindingRef>) -> R<Vec<ActionResult>> {
    let mut out = Vec::new();
    for it in items {
        let r = quarantine::add(&PathBuf::from(&it.path), &it.signature, &it.severity);
        if r.is_ok() {
            history::set_status(&it.scan_id, &it.id, "quarantined");
            update_last(&state, &it.id, "quarantined");
        }
        out.push(ActionResult { id: it.id, ok: r.is_ok(), error: r.err().unwrap_or_default() });
    }
    Ok(out)
}

#[tauri::command]
async fn delete_findings(state: State<'_, AppState>, items: Vec<FindingRef>) -> R<Vec<ActionResult>> {
    let mut out = Vec::new();
    for it in items {
        let r = std::fs::remove_file(&it.path).map_err(|e| e.to_string());
        if r.is_ok() {
            history::set_status(&it.scan_id, &it.id, "deleted");
            update_last(&state, &it.id, "deleted");
        }
        out.push(ActionResult { id: it.id, ok: r.is_ok(), error: r.err().unwrap_or_default() });
    }
    Ok(out)
}

#[tauri::command]
async fn ignore_findings(state: State<'_, AppState>, items: Vec<FindingRef>) -> R<Vec<ActionResult>> {
    let mut out = Vec::new();
    for it in items {
        let r = util::sha256_file(&PathBuf::from(&it.path)).map_err(|e| e.to_string());
        match r {
            Ok(sha) => {
                let name = PathBuf::from(&it.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let sig = it.signature.clone();
                mutate_settings(&state, |s| {
                    if !s.whitelist.iter().any(|w| w.sha256 == sha) {
                        s.whitelist.push(WhitelistEntry { sha256: sha, name, signature: sig, added: util::now_ms() });
                    }
                })?;
                history::set_status(&it.scan_id, &it.id, "ignored");
                update_last(&state, &it.id, "ignored");
                out.push(ActionResult { id: it.id, ok: true, error: String::new() });
            }
            Err(e) => out.push(ActionResult { id: it.id, ok: false, error: e }),
        }
    }
    Ok(out)
}

#[tauri::command]
fn whitelist_remove(state: State<'_, AppState>, sha256: String) -> R<Settings> {
    mutate_settings(&state, |s| s.whitelist.retain(|w| w.sha256 != sha256))
}

#[tauri::command]
fn reveal(path: String) -> R<()> {
    util::reveal(&PathBuf::from(path))
}

#[tauri::command]
fn open_url(url: String) -> R<()> {
    if !url.starts_with("https://") {
        return Err("only https".into());
    }
    let mut c = std::process::Command::new("xdg-open");
    util::clean_env(&mut c);
    c.arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

// ------------------------------------------------------------ quarantine

#[tauri::command]
fn quarantine_list() -> Vec<quarantine::QItem> {
    quarantine::list()
}

#[tauri::command]
async fn quarantine_restore(state: State<'_, AppState>, id: String, whitelist: bool) -> R<String> {
    let item = quarantine::sha_of(&id);
    let p = quarantine::restore(&id)?;
    if whitelist {
        if let Some(q) = item {
            let name = PathBuf::from(&q.original_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            mutate_settings(&state, |s| {
                if !s.whitelist.iter().any(|w| w.sha256 == q.sha256) {
                    s.whitelist.push(WhitelistEntry { sha256: q.sha256, name, signature: q.signature, added: util::now_ms() });
                }
            })?;
        }
    }
    Ok(p)
}

#[tauri::command]
fn quarantine_delete(ids: Vec<String>) -> R<()> {
    for id in ids {
        quarantine::delete(&id)?;
    }
    Ok(())
}

// ------------------------------------------------------------ history / reports

#[tauri::command]
fn history_list() -> Vec<ScanSummary> {
    history::list()
}

#[tauri::command]
fn history_remove(id: String) {
    history::remove(&id)
}

#[tauri::command]
fn history_clear() {
    history::clear()
}

#[tauri::command]
fn export_report(id: String, path: String, format: String) -> R<()> {
    let s = history::get(&id).ok_or("scan not found")?;
    let text = report::render(&s, &format)?;
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

// ------------------------------------------------------------ updates

#[tauri::command]
async fn check_update() -> R<updater::UpdateInfo> {
    tauri::async_runtime::spawn_blocking(updater::check).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn apply_update(app: AppHandle) -> R<()> {
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || updater::apply(&a)).await.map_err(|e| e.to_string())?
}

// ------------------------------------------------------------ main

fn startup(app: AppHandle) {
    let st = app.state::<AppState>();
    let s = st.settings.lock().unwrap().clone();
    let eng = st.engine.clone();
    let day = 20 * 3600 * 1000;
    let due = s.auto_update_signatures
        && s.onboarding_done
        && util::now_ms().saturating_sub(s.last_signature_update) > day;
    if s.preload_engine && engine::db_info().0 {
        let _ = eng.start(&app, &s);
    }
    if due && engine::locate().is_some() {
        spawn_signature_update(app.clone(), eng);
    }
}

fn main() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .filter(|a| std::path::Path::new(a).exists())
        .map(|a| std::fs::canonicalize(&a).map(|p| p.display().to_string()).unwrap_or(a))
        .collect();

    let state = AppState {
        settings: Mutex::new(Settings::load()),
        engine: Arc::new(Engine::new()),
        scan: Mutex::new(None),
        progress: Arc::new(Mutex::new(Progress::default())),
        last_summary: Mutex::new(history::list().into_iter().next()),
        startup_targets: Mutex::new(args),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(|app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || startup(handle));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            get_settings,
            save_settings,
            startup_targets,
            engine_status,
            engine_start,
            update_signatures,
            pick_files,
            pick_folders,
            pick_save,
            quick_targets,
            start_scan,
            self_test,
            pause_scan,
            cancel_scan,
            scan_state,
            clear_scan_cache,
            quarantine_findings,
            delete_findings,
            ignore_findings,
            whitelist_remove,
            reveal,
            open_url,
            quarantine_list,
            quarantine_restore,
            quarantine_delete,
            history_list,
            history_remove,
            history_clear,
            export_report,
            check_update,
            apply_update
        ])
        .build(tauri::generate_context!())
        .expect("error while building LakuenFileScanner")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(st) = app.try_state::<AppState>() {
                    if let Some(c) = st.scan.lock().unwrap().as_ref() {
                        c.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    st.engine.stop();
                }
            }
        });
}
