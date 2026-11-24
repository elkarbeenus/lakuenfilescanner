// Browser preview only: fake backend used when the UI runs outside Tauri.
(function () {
  const listeners = {};
  const emit = (ev, payload) => (listeners[ev] || []).forEach((cb) => cb({ payload }));
  let settings = {
    language: "de", theme: "midnight", threads: 0, max_file_mb: 1024, scan_archives: true, follow_symlinks: false, skip_hidden: false,
    heuristics: true, detect_pua: false, skip_unchanged: true, auto_update_signatures: true, preload_engine: true, check_app_updates: true,
    notify_on_finish: true, exclusions: ["/proc", "/sys", "~/.local/share/lakuenfilescanner"], excluded_extensions: [], whitelist: [],
    recent_targets: ["/home/deck/Downloads", "/run/media/deck/SD/Games"], onboarding_done: location.hash.includes("onb") ? false : true,
    last_signature_update: Date.now() - 3600e3,
  };
  const engine = { state: "ready", message: "", engine_version: "ClamAV 1.5.3", signature_version: "27832", signature_date: "Tue Nov 24 09:22:02 2026",
    db_present: true, db_size: 112e6, bundled: true, available: true, updating: false, threads: 16 };
  const findings = [
    { id: "f1", path: "/home/deck/Downloads/crack/Setup.exe", signature: "Win.Trojan.Agent-1234567", severity: "infected", engine: "clamav", size: 4.2e6, status: "open" },
    { id: "f2", path: "/home/deck/Downloads/Rechnung.pdf.exe", signature: "Lakuen.Heur.DoubleExtension", severity: "suspicious", engine: "lakuen", size: 812e3, status: "open" },
    { id: "f3", path: "/home/deck/.local/share/lakuenfilescanner/selftest/eicar-test.com", signature: "Eicar-Signature", severity: "infected", engine: "clamav", size: 68, status: "open" },
    { id: "f4", path: "/home/deck/Applications/tools/keygen.zip", signature: "PUA.Win.Tool.Keygen-42", severity: "pua", engine: "clamav", size: 1.3e6, status: "quarantined" },
  ];
  const last = { id: "s1", started: Date.now() - 400e3, finished: Date.now() - 300e3, duration_ms: 98000, targets: ["/home/deck/Downloads"], status: "completed",
    engine: "clamav", signature_version: "27832", files_scanned: 48211, bytes_scanned: 38.4e9, skipped: 12, cached: 30211, errors: 3, findings,
    error_list: [{ path: "/home/deck/.cache/x", message: "Permission denied" }] };
  const history = [last, { ...last, id: "s2", findings: [], finished: Date.now() - 86400e3 * 2, targets: ["/run/media/deck/SD/Games"], files_scanned: 210334, bytes_scanned: 412e9 }];
  let quarantine = [{ id: "q1", original_path: "/home/deck/Applications/tools/keygen.zip", signature: "PUA.Win.Tool.Keygen-42", severity: "pua", size: 1.3e6, sha256: "ab", mode: 420, quarantined_at: Date.now() - 7200e3 }];
  let scanTimer = null;

  function fakeScan() {
    const t0 = Date.now(); let done = 0; const total = 182340; let f = 0;
    emit("scan-started", {});
    scanTimer = setInterval(() => {
      done = Math.min(total, done + 4211);
      const p = { scan_id: "x", running: done < total, paused: false, cancelled: false, walk_done: done > 30000, files_total: done > 30000 ? total : done + 9000,
        bytes_total: 96e9, files_done: done, bytes_done: (done / total) * 96e9, skipped: 4, cached: Math.floor(done * 0.4), errors: 2, findings: f,
        current: "/run/media/deck/SD/Games/SteamLibrary/steamapps/common/Game/data/pak" + done + ".pak", elapsed_ms: Date.now() - t0, engine: "clamav", threads: 16 };
      if (done > 60000 && f < 2) { emit("scan-finding", findings[f]); f++; p.findings = f; }
      emit("scan-progress", p);
      if (done >= total) { clearInterval(scanTimer); emit("scan-done", { ...last, id: "s3", finished: Date.now(), findings: findings.slice(0, 2).map((x) => ({ ...x, status: "open" })) }); }
    }, 200);
  }

  const handlers = {
    app_info: () => ({ version: "0.1.0", label: "v0.1.0", repo: "elkarbeenus/lakuenfilescanner", appimage: true, cpus: 16, data_dir: "/home/deck/.local/share/lakuenfilescanner" }),
    get_settings: () => settings,
    save_settings: ({ settings: s }) => (settings = s),
    engine_status: () => engine,
    scan_state: () => ({ running: false, progress: {}, last }),
    history_list: () => history,
    quarantine_list: () => quarantine,
    startup_targets: () => [],
    quick_targets: () => ["/home/deck/Downloads", "/tmp"],
    pick_files: () => ["/home/deck/Downloads/a.exe"],
    pick_folders: () => ["/run/media/deck/SD/Games"],
    start_scan: () => { fakeScan(); return "x"; },
    self_test: () => { fakeScan(); return "x"; },
    check_update: () => ({ available: location.hash.includes("upd"), current: "0.1.0", current_label: "v0.1.0", latest: "v0.1.1", latest_label: "v0.1.1", notes: "- Neu: …", can_self_update: true }),
    quarantine_findings: ({ items }) => items.map((i) => ({ id: i.id, ok: true, error: "" })),
    delete_findings: ({ items }) => items.map((i) => ({ id: i.id, ok: true, error: "" })),
    ignore_findings: ({ items }) => items.map((i) => ({ id: i.id, ok: true, error: "" })),
  };
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => (handlers[cmd] ? handlers[cmd](args || {}) : null) },
    event: { listen: async (ev, cb) => { (listeners[ev] = listeners[ev] || []).push(cb); return () => {}; } },
  };
  window.__mockEmit = emit;
})();
