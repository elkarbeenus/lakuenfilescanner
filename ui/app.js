// LakuenFileScanner – erstellt von Dominic Lesina x Claude
"use strict";

(async function boot() {
  if (!window.__TAURI__) {
    await new Promise((res) => { const s = document.createElement("script"); s.src = "mock.js"; s.onload = res; document.head.appendChild(s); });
  }
  start();
})();

function start() {
const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const listen = (ev, cb) => T.event.listen(ev, (e) => cb(e.payload));

// ------------------------------------------------------------------ state
const S = {
  view: "dashboard", settings: null, info: {}, engine: {}, progress: null, running: false,
  live: [], last: null, results: null, history: [], quarantine: [], update: null, updating: null,
  freshLog: [], sel: new Set(), filter: { q: "", sev: "all", open: false }, speed: [],
};

// ------------------------------------------------------------------ helpers
const $ = (s, r = document) => r.querySelector(s);
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const ic = (n, cls = "") => `<svg class="i ${cls}"><use href="#i-${n}"/></svg>`;
const lang = () => (S.settings?.language === "en" ? "en" : "de");
function t(k, v = {}) {
  let s = (I18N[lang()] && I18N[lang()][k]) ?? I18N.de[k] ?? k;
  for (const [a, b] of Object.entries(v)) s = s.split(`{${a}}`).join(b);
  return s;
}
const loc = () => (lang() === "en" ? "en-GB" : "de-DE");
const num = (n) => Number(n || 0).toLocaleString(loc());
function bytes(b) {
  const u = ["B", "KB", "MB", "GB", "TB"]; let v = Number(b || 0), i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v.toLocaleString(loc(), { maximumFractionDigits: v < 10 && i ? 1 : 0 })} ${u[i]}`;
}
function dur(ms) {
  const s = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), x = s % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(x).padStart(2, "0")}` : `${m}:${String(x).padStart(2, "0")}`;
}
const date = (ms) => new Date(ms).toLocaleString(loc(), { dateStyle: "medium", timeStyle: "short" });
function ago(ms) {
  const d = (Date.now() - ms) / 1000;
  const rtf = new Intl.RelativeTimeFormat(loc(), { numeric: "auto" });
  if (d < 60) return rtf.format(-Math.round(d), "second");
  if (d < 3600) return rtf.format(-Math.round(d / 60), "minute");
  if (d < 86400) return rtf.format(-Math.round(d / 3600), "hour");
  return rtf.format(-Math.round(d / 86400), "day");
}
const MON = { Jan: 0, Feb: 1, Mar: 2, Apr: 3, May: 4, Jun: 5, Jul: 6, Aug: 7, Sep: 8, Oct: 9, Nov: 10, Dec: 11 };
function sigDate(s) {
  const m = /(\w{3})\s+(\d{1,2})\s+(\d{1,2}):(\d{2}):(\d{2})\s+(\d{4})/.exec(s || "");
  if (!m || MON[m[1]] === undefined) return s || "–";
  return date(new Date(+m[6], MON[m[1]], +m[2], +m[3], +m[4]).getTime());
}
const base = (p) => String(p).split("/").filter(Boolean).pop() || p;
const dir = (p) => { const a = String(p).split("/"); a.pop(); return a.join("/") || "/"; };

function toast(msg, type = "ok") {
  const el = document.createElement("div");
  el.className = `toast ${type}`;
  el.innerHTML = `${ic(type === "bad" ? "alert" : type === "info" ? "info" : "check")}<span>${esc(msg)}</span>`;
  $("#toasts").appendChild(el);
  setTimeout(() => { el.style.transition = ".3s"; el.style.opacity = "0"; setTimeout(() => el.remove(), 300); }, type === "bad" ? 6000 : 3200);
}

function modal(html, { wide = false, onClose } = {}) {
  const root = $("#modal");
  root.innerHTML = `<div class="overlay"><div class="modal ${wide ? "wide" : ""}">${html}</div></div>`;
  const ov = root.firstElementChild;
  const close = () => { root.innerHTML = ""; onClose && onClose(); };
  ov.addEventListener("mousedown", (e) => { if (e.target === ov) close(); });
  return { el: ov.firstElementChild, close };
}

function confirmDialog({ title, text = "", ok = t("confirm"), danger = false }) {
  return new Promise((res) => {
    const m = modal(`<h2>${esc(title)}</h2><p>${esc(text)}</p>
      <div class="foot"><button class="btn ghost" data-a="no">${t("cancel")}</button>
      <button class="btn ${danger ? "danger solid" : "primary"}" data-a="yes">${esc(ok)}</button></div>`, { onClose: () => res(false) });
    m.el.querySelector('[data-a="no"]').onclick = () => { m.close(); };
    m.el.querySelector('[data-a="yes"]').onclick = () => { $("#modal").innerHTML = ""; res(true); };
    m.el.querySelector('[data-a="yes"]').focus();
  });
}

function errText(e) {
  const s = String(e?.message ?? e);
  if (s === "busy") return t("err_busy");
  if (s === "no targets") return t("err_no_targets");
  return s;
}

// ------------------------------------------------------------------ i18n / theme
function applyStatic() {
  document.documentElement.lang = lang();
  document.documentElement.dataset.theme = S.settings?.theme || "midnight";
  document.querySelectorAll("[data-i18n]").forEach((el) => (el.textContent = t(el.dataset.i18n)));
  $("#verLabel").textContent = S.info.label || "";
}

// ------------------------------------------------------------------ navigation
const NAV = [
  ["dashboard", "home"], ["scan", "scan"], ["results", "list"], ["quarantine", "box"],
  ["history", "clock"], ["settings", "cog"], ["about", "info"],
];
function renderNav() {
  const open = (S.last?.findings || []).filter((f) => f.status === "open").length;
  $("#nav").innerHTML = NAV.map(([k, i]) => {
    let badge = "";
    if (k === "scan" && S.running) badge = `<span class="badge live">●</span>`;
    if (k === "results" && open) badge = `<span class="badge">${open}</span>`;
    if (k === "quarantine" && S.quarantine.length) badge = `<span class="badge" style="background:var(--panel2);color:var(--muted)">${S.quarantine.length}</span>`;
    return `<button class="${S.view === k ? "active" : ""}" data-v="${k}">${ic(i)}<span>${t("nav_" + k)}</span>${badge}</button>`;
  }).join("");
  $("#nav").querySelectorAll("button").forEach((b) => (b.onclick = () => go(b.dataset.v)));
}
function go(v, opts = {}) {
  S.view = v;
  if (v === "results" && !opts.keep) { S.results = S.last; S.sel.clear(); }
  render();
  $("#main").scrollTop = 0;
}

// ------------------------------------------------------------------ engine status
function engineView() {
  const e = S.engine || {};
  if (e.updating) return { key: "updating", color: "var(--accent)", icon: "refresh", spin: true, dot: "busy" };
  switch (e.state) {
    case "ready": {
      const old = S.settings?.last_signature_update && Date.now() - S.settings.last_signature_update > 3 * 86400e3;
      return old ? { key: "old", color: "var(--warn)", icon: "shield-alert", dot: "warn" } : { key: "protected", color: "var(--ok)", icon: "shield-ok", dot: "ok" };
    }
    case "starting": return { key: "starting", color: "var(--accent)", icon: "shield", spin: true, dot: "busy" };
    case "no_db": return { key: "nodb", color: "var(--warn)", icon: "shield-alert", dot: "warn" };
    case "missing": return { key: "missing", color: "var(--info)", icon: "shield", dot: "warn" };
    case "error": return { key: "error", color: "var(--bad)", icon: "shield-alert", dot: "bad" };
    default: return { key: "stopped", color: "var(--muted)", icon: "shield", dot: "" };
  }
}
function renderPill() {
  const v = engineView();
  $("#enginePill").innerHTML = `<span class="dot ${v.dot}"></span><span>${esc(t("st_" + v.key))}</span>`;
}

// ------------------------------------------------------------------ actions
async function pickAndScan(kind) {
  try {
    const paths = await invoke(kind === "files" ? "pick_files" : "pick_folders");
    if (paths && paths.length) startScan(paths);
  } catch (e) { toast(errText(e), "bad"); }
}
async function quickScan() { startScan(await invoke("quick_targets")); }
async function startScan(targets) {
  if (S.running) return toast(t("err_busy"), "info");
  try {
    S.live = []; S.speed = []; S.progress = null;
    await invoke("start_scan", { targets });
    S.running = true;
    go("scan");
  } catch (e) { toast(errText(e), "bad"); }
}
async function selfTest() {
  if (S.running) return toast(t("err_busy"), "info");
  try { S.live = []; S.speed = []; await invoke("self_test"); S.running = true; go("scan"); } catch (e) { toast(errText(e), "bad"); }
}
async function cancelScan() {
  if (!S.running) return;
  if (await confirmDialog({ title: t("confirm_cancel"), text: t("confirm_cancel_sub"), ok: t("yes_cancel"), danger: true })) invoke("cancel_scan");
}
async function saveSettings(patch, quiet) {
  const next = { ...S.settings, ...patch };
  try {
    S.settings = await invoke("save_settings", { settings: next });
    applyStatic();
    if (!quiet) toast(t("saved"));
  } catch (e) { toast(errText(e), "bad"); }
}
const updateSigs = () => { S.freshLog = []; invoke("update_signatures"); S.engine = { ...S.engine, updating: true }; renderPill(); render(); };

// ------------------------------------------------------------------ views
function render() {
  renderNav(); renderPill(); renderBanner();
  const v = { dashboard: vDashboard, scan: vScan, results: vResults, quarantine: vQuarantine, history: vHistory, settings: vSettings, about: vAbout }[S.view];
  $("#view").innerHTML = `<div class="view">${v()}</div>`;
  bind[S.view] && bind[S.view]();
}
const bind = {};

function tiles() {
  return `<div class="grid g4">
    <button class="tile" data-act="files"><div class="ic">${ic("file")}</div><div><b>${t("act_files")}</b><br><span>${t("act_files_sub")}</span></div></button>
    <button class="tile" data-act="folder"><div class="ic">${ic("folder")}</div><div><b>${t("act_folder")}</b><br><span>${t("act_folder_sub")}</span></div></button>
    <button class="tile" data-act="quick"><div class="ic">${ic("zap")}</div><div><b>${t("act_quick")}</b><br><span>${t("act_quick_sub")}</span></div></button>
    <button class="tile" data-act="selftest"><div class="ic">${ic("flask")}</div><div><b>${t("act_selftest")}</b><br><span>${t("act_selftest_sub")}</span></div></button>
  </div>
  <div class="drop">${ic("download")}<div><b>${t("drop_title")}</b>${t("drop_sub")}</div></div>`;
}
function bindTiles() {
  document.querySelectorAll("[data-act]").forEach((b) => (b.onclick = () => ({ files: () => pickAndScan("files"), folder: () => pickAndScan("folder"), quick: quickScan, selftest: selfTest }[b.dataset.act]())));
}

function vDashboard() {
  const e = S.engine || {}, v = engineView();
  const sub = t("st_" + v.key + "_sub", { sig: e.signature_version || "–", date: sigDate(e.signature_date), msg: e.message || "" });
  let btns = "";
  if (v.key === "nodb") btns = `<button class="btn primary" data-x="sigs">${ic("download")}${t("btn_download_sigs")}</button>`;
  else if (v.key === "old" || v.key === "protected") btns = `<button class="btn" data-x="sigs">${ic("refresh")}${t("btn_update_sigs")}</button>`;
  else if (v.key === "error" || v.key === "stopped") btns = `<button class="btn" data-x="start">${ic("play")}${t("btn_start_engine")}</button>`;
  const last = S.last;
  const recent = (S.settings?.recent_targets || []);
  return `
    <div class="card hero" style="--st:${v.color}">
      <div class="shield ${v.spin ? "spin" : ""}">${ic(v.icon)}</div>
      <div><h2>${t("st_" + v.key)}</h2><p>${esc(sub)}</p><div class="row">${btns}</div></div>
    </div>
    <div class="grid g4" style="margin-top:14px">
      <div class="card stat"><span class="k">${t("card_signatures")}</span><span class="v">${e.signature_version ? num(e.signature_version) : "–"}</span><span class="s">${esc(e.signature_date ? sigDate(e.signature_date) : (e.db_size ? bytes(e.db_size) : "–"))}</span></div>
      <div class="card stat"><span class="k">${t("card_lastscan")}</span><span class="v">${last ? ago(last.finished) : t("never")}</span><span class="s">${last ? (last.findings.length ? t("findings_short", { n: last.findings.length }) : t("no_findings_short")) + " · " + num(last.files_scanned) + " " + t("m_files") : "&nbsp;"}</span></div>
      <div class="card stat"><span class="k">${t("card_quarantine")}</span><span class="v">${num(S.quarantine.length)}</span><span class="s">${t("items", { n: S.quarantine.length })}</span></div>
      <div class="card stat"><span class="k">${t("card_engine")}</span><span class="v">${esc((e.engine_version || "Lakuen").replace("ClamAV ", "ClamAV "))}</span><span class="s">${t("threads", { n: e.threads || S.info.cpus || "–" })}</span></div>
    </div>
    <div style="margin-top:22px">${tiles()}</div>
    ${recent.length ? `<h3 style="margin:22px 0 10px" class="muted">${t("recent")}</h3><div class="chips">${recent.map((r) => `<button class="chip" data-recent="${esc(r)}" title="${esc(r)}">${ic("refresh")}${esc(r)}</button>`).join("")}</div>` : ""}`;
}
bind.dashboard = () => {
  bindTiles();
  document.querySelectorAll("[data-recent]").forEach((b) => (b.onclick = () => startScan([b.dataset.recent])));
  const x = (k, f) => { const b = document.querySelector(`[data-x="${k}"]`); if (b) b.onclick = f; };
  x("sigs", updateSigs);
  x("start", () => invoke("engine_start"));
};

function vScan() {
  const p = S.progress;
  if (!S.running && !p) {
    return `<div class="head"><div><h1>${t("nav_scan")}</h1></div></div>
      <div class="card empty"><div class="big">${ic("scan")}</div><h2>${t("scan_idle_title")}</h2><div>${t("scan_idle_sub")}</div></div>
      <div style="margin-top:14px">${tiles()}</div>`;
  }
  return `<div class="head"><div><h1 id="scTitle"></h1><div class="sub" id="scSub"></div></div><div class="row" id="scBtns" style="display:flex;gap:10px"></div></div>
    <div class="card">
      <div class="scanhead">
        <div class="ring" id="ring"><svg viewBox="0 0 120 120"><circle class="bg" cx="60" cy="60" r="52"/><circle class="fg" id="ringFg" cx="60" cy="60" r="52" stroke-dasharray="326.7" stroke-dashoffset="326.7"/></svg><div class="pct" id="pct">0%</div></div>
        <div style="flex:1;min-width:0">
          <div class="bar"><i id="bar"></i></div>
          <div class="current mono" id="cur">&nbsp;</div>
          <div id="engWarn"></div>
        </div>
      </div>
      <div class="metrics">
        ${[["files", "m_files"], ["data", "m_data"], ["speed", "m_speed"], ["rate", "m_rate"], ["find", "m_findings"], ["elapsed", "m_elapsed"], ["eta", "m_eta"], ["skip", "m_skipped"], ["err", "m_errors"], ["thr", "m_threads"]]
          .map(([id, k]) => `<div class="metric" id="mm_${id}"><div class="k">${t(k)}</div><div class="v" id="m_${id}">–</div>${id === "files" ? `<div class="dim" style="font-size:12px" id="m_files_sub"></div>` : ""}</div>`).join("")}
      </div>
    </div>
    <h2 style="margin:22px 0 10px">${t("live_findings")}</h2>
    <div class="tablewrap"><table><thead><tr><th>${t("col_threat")}</th><th>${t("col_path")}</th><th>${t("col_size")}</th></tr></thead><tbody id="liveBody"></tbody></table></div>`;
}
bind.scan = () => {
  if (!S.running && !S.progress) return bindTiles();
  updateScanView();
  $("#liveBody").innerHTML = S.live.map(liveRow).join("") || emptyLive();
};
const emptyLive = () => `<tr id="liveEmpty"><td colspan="3" class="muted" style="text-align:center;padding:22px">–</td></tr>`;
const sevBadge = (s) => `<span class="sev ${esc(s)}">${ic(s === "suspicious" ? "alert" : "shield-alert")}${t("sev_" + s)}</span>`;
const liveRow = (f) => `<tr><td class="threat">${sevBadge(f.severity)} <div class="sig" style="margin-top:4px">${esc(f.signature)}</div></td><td class="path"><div class="mono">${esc(base(f.path))}</div><small class="mono">${esc(dir(f.path))}</small></td><td>${bytes(f.size)}</td></tr>`;

function updateScanView() {
  const p = S.progress; if (!p || !$("#ring")) return;
  const now = Date.now();
  S.speed.push({ t: now, b: p.bytes_done, f: p.files_done });
  while (S.speed.length > 2 && now - S.speed[0].t > 4000) S.speed.shift();
  const a = S.speed[0], z = S.speed[S.speed.length - 1];
  const dt = (z.t - a.t) / 1000;
  const bps = dt > 0.3 ? (z.b - a.b) / dt : p.elapsed_ms ? p.bytes_done / (p.elapsed_ms / 1000) : 0;
  const fps = dt > 0.3 ? (z.f - a.f) / dt : p.elapsed_ms ? p.files_done / (p.elapsed_ms / 1000) : 0;
  const total = Math.max(p.files_total, p.files_done);
  let pct = total ? (p.files_done / total) : 0;
  if (p.bytes_total > 0 && p.walk_done) pct = Math.max(0, Math.min(1, 0.5 * (p.files_done / total) + 0.5 * (p.bytes_done / p.bytes_total)));
  if (!p.running) pct = p.cancelled ? pct : 1;
  const indet = p.running && !p.walk_done && total < 50;
  $("#ring").classList.toggle("indet", indet);
  $("#ringFg").style.strokeDashoffset = indet ? 326.7 * 0.75 : 326.7 * (1 - pct);
  $("#pct").textContent = indet ? "…" : `${Math.floor(pct * 100)}%`;
  $("#bar").style.width = `${pct * 100}%`;
  $("#cur").innerHTML = p.running && p.current ? `<span dir="ltr">${esc(p.current)}</span>` : "&nbsp;";
  let title = p.running ? (p.paused ? t("scan_paused") : t("scan_running")) : p.cancelled ? t("scan_cancelled") : t("scan_done");
  $("#scTitle").innerHTML = p.running ? `<span class="grad">${title}</span>` : title;
  $("#scSub").textContent = p.running && !p.walk_done ? t("scan_counting") : "";
  const remainB = p.bytes_total - p.bytes_done;
  const eta = p.running && p.walk_done && bps > 0 ? dur((remainB / bps) * 1000) : p.running ? "…" : "–";
  const set = (id, v) => { const el = $("#m_" + id); if (el) el.textContent = v; };
  set("files", num(p.files_done)); { const el = $("#m_files_sub"); if (el) el.textContent = p.walk_done || !p.running ? `/ ${num(total)}` : `/ ${num(total)} …`; }
  set("data", `${bytes(p.bytes_done)}`);
  set("speed", p.running ? `${bytes(bps)}/s` : `${bytes(p.elapsed_ms ? p.bytes_done / (p.elapsed_ms / 1000) : 0)}/s`);
  set("rate", num(Math.round(p.running ? fps : p.elapsed_ms ? p.files_done / (p.elapsed_ms / 1000) : 0)));
  set("find", num(p.findings)); $("#mm_find").classList.toggle("bad", p.findings > 0);
  set("elapsed", dur(p.elapsed_ms)); set("eta", eta);
  set("skip", num(p.skipped + p.cached)); set("err", num(p.errors)); set("thr", p.threads);
  $("#engWarn").innerHTML = p.engine === "lakuen" ? `<div class="muted" style="margin-top:10px;display:flex;gap:8px;align-items:center">${ic("info")}${t("engine_lakuen_only")}</div>` : "";
  const btns = $("#scBtns");
  const want = p.running ? (p.paused ? "r" : "p") : "d";
  if (btns.dataset.m !== want) {
    btns.dataset.m = want;
    btns.innerHTML = p.running
      ? `<button class="btn" id="bPause">${ic(p.paused ? "play" : "pause")}${t(p.paused ? "resume" : "pause")}</button><button class="btn danger" id="bCancel">${ic("x")}${t("cancel")}</button>`
      : `<button class="btn" id="bNew">${ic("scan")}${t("new_scan")}</button><button class="btn primary" id="bRes">${ic("list")}${t("show_results")}</button>`;
    if (p.running) {
      $("#bPause").onclick = () => invoke("pause_scan", { paused: !S.progress.paused });
      $("#bCancel").onclick = cancelScan;
    } else {
      $("#bNew").onclick = () => { S.progress = null; render(); };
      $("#bRes").onclick = () => go("results");
    }
  }
}

// ---------------- results
function filtered() {
  const r = S.results; if (!r) return [];
  const q = S.filter.q.toLowerCase();
  return r.findings.filter((f) => (S.filter.sev === "all" || f.severity === S.filter.sev)
    && (!S.filter.open || f.status === "open")
    && (!q || f.path.toLowerCase().includes(q) || f.signature.toLowerCase().includes(q)));
}
function vResults() {
  const r = S.results;
  if (!r) return `<div class="head"><h1>${t("res_title")}</h1></div><div class="card empty"><div class="big">${ic("list")}</div><h2>${t("res_none")}</h2><div>${t("res_none_sub")}</div></div>`;
  const meta = t("res_meta", { date: date(r.finished), dur: dur(r.duration_ms), files: num(r.files_scanned), data: bytes(r.bytes_scanned) });
  const head = `<div class="head"><div><h1>${t("res_title")}</h1><div class="sub">${esc(meta)} · <span class="tag">${esc(r.engine === "clamav" ? "ClamAV " + (r.signature_version || "") : "Lakuen")}</span> ${r.status === "cancelled" ? `<span class="tag">${t("cancelled")}</span>` : ""}</div>
    <div class="sub mono" style="margin-top:6px">${r.targets.map(esc).join(" · ")}</div></div>
    <div style="display:flex;gap:8px"><select class="input" id="expFmt"><option value="html">HTML</option><option value="json">JSON</option><option value="txt">TXT</option></select><button class="btn" id="bExport">${ic("download")}${t("export")}</button></div></div>`;
  const errs = r.error_list?.length ? `<details class="errors"><summary>${t("errors_title", { n: num(r.errors) })}</summary><div class="list mono">${r.error_list.slice(0, 300).map((e) => `<div>${esc(e.path)} — <span class="dim">${esc(e.message)}</span></div>`).join("")}</div></details>` : "";
  if (!r.findings.length) {
    return head + `<div class="card empty clean"><div class="big">${ic("shield-ok")}</div><h2>${t("res_clean")}</h2><div>${t("res_clean_sub", { files: num(r.files_scanned), data: bytes(r.bytes_scanned) })}</div></div>` + errs;
  }
  const counts = { all: r.findings.length };
  r.findings.forEach((f) => (counts[f.severity] = (counts[f.severity] || 0) + 1));
  const chips = ["all", "infected", "pua", "suspicious"].filter((k) => k === "all" || counts[k])
    .map((k) => `<button class="chip ${S.filter.sev === k ? "on" : ""}" data-sev="${k}">${k === "all" ? t("all") : t("sev_" + k)} · ${counts[k]}</button>`).join("");
  return head + `
    <div class="toolbar">
      <div class="search">${ic("search")}<input class="input" id="q" placeholder="${t("search")}" value="${esc(S.filter.q)}"></div>
      <div class="chips">${chips}<button class="chip ${S.filter.open ? "on" : ""}" id="openOnly">${t("open_only")}</button></div>
    </div>
    <div id="bulk"></div>
    <div class="tablewrap"><table>
      <thead><tr><th style="width:1%"><input type="checkbox" id="selAll"></th><th>${t("col_threat")}</th><th>${t("col_path")}</th><th>${t("col_size")}</th><th>${t("col_status")}</th><th></th></tr></thead>
      <tbody id="resBody"></tbody></table></div>` + errs;
}
function resRow(f) {
  const done = f.status !== "open";
  return `<tr class="${done ? "done" : ""}" data-id="${esc(f.id)}">
    <td><input type="checkbox" class="sel" ${S.sel.has(f.id) ? "checked" : ""} ${done ? "disabled" : ""}></td>
    <td class="threat">${sevBadge(f.severity)}<div class="sig" style="margin-top:4px">${esc(f.signature)}</div><span class="tag">${f.engine === "clamav" ? "ClamAV" : "Lakuen"}</span></td>
    <td class="path"><div class="mono" title="${esc(f.path)}">${esc(base(f.path))}</div><small class="mono" title="${esc(f.path)}">${esc(dir(f.path))}</small></td>
    <td>${bytes(f.size)}</td>
    <td><span class="tag">${t("status_" + f.status)}</span></td>
    <td class="actions">
      ${done ? "" : `<button class="icon-btn" data-a="quarantine" title="${t("a_quarantine")}">${ic("lock")}</button>
      <button class="icon-btn bad" data-a="delete" title="${t("a_delete")}">${ic("trash")}</button>
      <button class="icon-btn" data-a="ignore" title="${t("a_ignore")}">${ic("eye-off")}</button>`}
      <button class="icon-btn" data-a="reveal" title="${t("a_reveal")}">${ic("external")}</button>
      <button class="icon-btn" data-a="copy" title="${t("a_copy")}">${ic("copy")}</button>
    </td></tr>`;
}
function renderResBody() {
  const body = $("#resBody"); if (!body) return;
  const list = filtered();
  body.innerHTML = list.map(resRow).join("") || `<tr><td colspan="6" class="muted" style="text-align:center;padding:22px">–</td></tr>`;
  body.querySelectorAll("tr[data-id]").forEach((tr) => {
    const f = S.results.findings.find((x) => x.id === tr.dataset.id);
    const cb = tr.querySelector(".sel");
    cb.onchange = () => { cb.checked ? S.sel.add(f.id) : S.sel.delete(f.id); renderBulk(); };
    tr.querySelectorAll("[data-a]").forEach((b) => (b.onclick = () => act(b.dataset.a, [f])));
  });
  const open = list.filter((f) => f.status === "open");
  $("#selAll").checked = open.length > 0 && open.every((f) => S.sel.has(f.id));
  renderBulk();
}
function renderBulk() {
  const el = $("#bulk"); if (!el) return;
  const items = S.results.findings.filter((f) => S.sel.has(f.id) && f.status === "open");
  el.innerHTML = items.length ? `<div class="bulk"><b>${t("selected", { n: items.length })}</b><span style="flex:1"></span>
    <button class="btn sm" data-b="quarantine">${ic("lock")}${t("a_quarantine")}</button>
    <button class="btn sm danger" data-b="delete">${ic("trash")}${t("a_delete")}</button>
    <button class="btn sm" data-b="ignore">${ic("eye-off")}${t("a_ignore")}</button></div>` : "";
  el.querySelectorAll("[data-b]").forEach((b) => (b.onclick = () => act(b.dataset.b, items)));
}
async function act(kind, items) {
  if (kind === "reveal") return invoke("reveal", { path: items[0].path }).catch((e) => toast(errText(e), "bad"));
  if (kind === "copy") {
    try { await navigator.clipboard.writeText(items[0].path); } catch { const ta = document.createElement("textarea"); ta.value = items[0].path; document.body.appendChild(ta); ta.select(); document.execCommand("copy"); ta.remove(); }
    return toast(t("copied"));
  }
  if (kind === "delete" && !(await confirmDialog({ title: t("confirm_delete", { n: items.length }), text: t("confirm_delete_sub"), ok: t("yes_delete"), danger: true }))) return;
  if (kind === "ignore" && !(await confirmDialog({ title: t("confirm_ignore", { n: items.length }), text: t("confirm_ignore_sub"), ok: t("a_ignore") }))) return;
  const refs = items.map((f) => ({ scan_id: S.results.id, id: f.id, path: f.path, signature: f.signature, severity: f.severity }));
  const cmd = { quarantine: "quarantine_findings", delete: "delete_findings", ignore: "ignore_findings" }[kind];
  const status = { quarantine: "quarantined", delete: "deleted", ignore: "ignored" }[kind];
  try {
    const res = await invoke(cmd, { items: refs });
    const okIds = new Set(res.filter((r) => r.ok).map((r) => r.id));
    const bad = res.filter((r) => !r.ok);
    for (const list of [S.results?.findings, S.last?.findings]) (list || []).forEach((f) => { if (okIds.has(f.id)) f.status = status; });
    okIds.forEach((id) => S.sel.delete(id));
    if (okIds.size) toast(t("done_" + kind, { n: okIds.size }));
    if (bad.length) toast(t("failed_n", { n: bad.length, msg: bad[0].error }), "bad");
    if (kind === "quarantine") S.quarantine = await invoke("quarantine_list");
    if (kind === "ignore") S.settings = await invoke("get_settings");
    renderNav(); renderResBody();
  } catch (e) { toast(errText(e), "bad"); }
}
bind.results = () => {
  if (!S.results) return;
  const ex = $("#bExport");
  if (ex) ex.onclick = async () => {
    const fmt = $("#expFmt").value;
    const d = new Date(S.results.finished).toISOString().slice(0, 16).replace(/[:T]/g, "-");
    const path = await invoke("pick_save", { name: `lakuen-scan-${d}.${fmt}`, ext: fmt });
    if (!path) return;
    try { await invoke("export_report", { id: S.results.id, path, format: fmt }); toast(path); } catch (e) { toast(errText(e), "bad"); }
  };
  if (!S.results.findings.length) return;
  $("#q").oninput = (e) => { S.filter.q = e.target.value; renderResBody(); };
  document.querySelectorAll("[data-sev]").forEach((b) => (b.onclick = () => { S.filter.sev = b.dataset.sev; render(); }));
  $("#openOnly").onclick = () => { S.filter.open = !S.filter.open; render(); };
  $("#selAll").onchange = (e) => { filtered().filter((f) => f.status === "open").forEach((f) => (e.target.checked ? S.sel.add(f.id) : S.sel.delete(f.id))); renderResBody(); };
  renderResBody();
};

// ---------------- quarantine
function vQuarantine() {
  const q = S.quarantine;
  const head = `<div class="head"><div><h1>${t("q_title")}</h1><div class="sub">${t("q_sub")}</div></div>
    ${q.length ? `<button class="btn danger" id="qAll">${ic("trash")}${t("q_delete_all")}</button>` : ""}</div>`;
  if (!q.length) return head + `<div class="card empty"><div class="big">${ic("box")}</div><h2>${t("q_empty")}</h2></div>`;
  return head + `<div class="tablewrap"><table><thead><tr><th>${t("col_threat")}</th><th>${t("col_path")}</th><th>${t("col_size")}</th><th>${t("q_date")}</th><th></th></tr></thead><tbody>
    ${q.map((x) => `<tr data-id="${esc(x.id)}"><td class="threat">${sevBadge(x.severity)}<div class="sig" style="margin-top:4px">${esc(x.signature)}</div></td>
      <td class="path"><div class="mono" title="${esc(x.original_path)}">${esc(base(x.original_path))}</div><small class="mono">${esc(dir(x.original_path))}</small></td>
      <td>${bytes(x.size)}</td><td>${date(x.quarantined_at)}</td>
      <td class="actions"><button class="btn sm" data-a="restore">${ic("undo")}${t("q_restore")}</button>
      <button class="icon-btn" data-a="restore_ignore" title="${t("q_restore_ignore")}">${ic("eye-off")}</button>
      <button class="icon-btn bad" data-a="delete" title="${t("a_delete")}">${ic("trash")}</button></td></tr>`).join("")}
  </tbody></table></div>`;
}
bind.quarantine = () => {
  const refresh = async () => { S.quarantine = await invoke("quarantine_list"); render(); };
  const all = $("#qAll");
  if (all) all.onclick = async () => {
    if (!(await confirmDialog({ title: t("confirm_q_delete", { n: S.quarantine.length }), ok: t("yes_delete"), danger: true }))) return;
    await invoke("quarantine_delete", { ids: S.quarantine.map((x) => x.id) }); refresh();
  };
  document.querySelectorAll("#view tr[data-id]").forEach((tr) => tr.querySelectorAll("[data-a]").forEach((b) => (b.onclick = async () => {
    const id = tr.dataset.id;
    try {
      if (b.dataset.a === "delete") {
        if (!(await confirmDialog({ title: t("confirm_q_delete", { n: 1 }), ok: t("yes_delete"), danger: true }))) return;
        await invoke("quarantine_delete", { ids: [id] });
      } else {
        const p = await invoke("quarantine_restore", { id, whitelist: b.dataset.a === "restore_ignore" });
        toast(t("restored_to", { p }));
        if (b.dataset.a === "restore_ignore") S.settings = await invoke("get_settings");
      }
      refresh();
    } catch (e) { toast(errText(e), "bad"); }
  })));
};

// ---------------- history
function vHistory() {
  const h = S.history;
  const head = `<div class="head"><div><h1>${t("h_title")}</h1></div>${h.length ? `<button class="btn ghost" id="hClear">${ic("trash")}${t("h_clear")}</button>` : ""}</div>`;
  if (!h.length) return head + `<div class="card empty"><div class="big">${ic("clock")}</div><h2>${t("h_empty")}</h2></div>`;
  return head + `<div class="card" style="padding:4px 0">${h.map((s) => {
    const n = s.findings.length, cls = s.status === "cancelled" ? "cancel" : n ? "bad" : "";
    return `<div class="hitem" data-id="${esc(s.id)}"><div class="hicon ${cls}">${ic(s.status === "cancelled" ? "x" : n ? "shield-alert" : "shield-ok")}</div>
      <div style="min-width:0"><div class="t mono">${esc(s.targets.join(" · "))}</div>
      <div class="muted" style="font-size:12.5px">${date(s.finished)} · ${dur(s.duration_ms)} · ${num(s.files_scanned)} ${t("m_files")} · ${bytes(s.bytes_scanned)} · ${n ? t("findings_short", { n }) : t("no_findings_short")} · ${t(s.status)}</div></div>
      <div style="display:flex;gap:6px"><button class="btn sm" data-a="open">${t("h_open")}</button><button class="icon-btn" data-a="rescan" title="${t("new_scan")}">${ic("refresh")}</button><button class="icon-btn bad" data-a="remove" title="${t("h_remove")}">${ic("trash")}</button></div></div>`;
  }).join("")}</div>`;
}
bind.history = () => {
  const c = $("#hClear");
  if (c) c.onclick = async () => { if (await confirmDialog({ title: t("confirm_h_clear"), ok: t("h_clear"), danger: true })) { await invoke("history_clear"); S.history = []; render(); } };
  document.querySelectorAll(".hitem").forEach((row) => row.querySelectorAll("[data-a]").forEach((b) => (b.onclick = async () => {
    const s = S.history.find((x) => x.id === row.dataset.id);
    if (b.dataset.a === "open") { S.results = s; S.sel.clear(); go("results", { keep: true }); }
    if (b.dataset.a === "rescan") startScan(s.targets);
    if (b.dataset.a === "remove") { await invoke("history_remove", { id: s.id }); S.history = S.history.filter((x) => x.id !== s.id); render(); }
  })));
};

// ---------------- settings
const THEMES = [
  ["midnight", "Midnight", "#0a0e1c", "#22d3ee", "#8b5cf6"], ["obsidian", "Obsidian", "#09090b", "#34d399", "#10b981"],
  ["crimson", "Crimson", "#0e0809", "#fb7185", "#f97316"], ["synthwave", "Synthwave", "#120a1f", "#f472b6", "#a855f7"],
  ["nord", "Nord", "#1e222b", "#88c0d0", "#81a1c1"], ["light", "Light", "#f3f5fb", "#0891b2", "#7c3aed"],
];
const sw = (key, label, sub) => `<div class="srow"><div class="l"><b>${label}</b>${sub ? `<span>${sub}</span>` : ""}</div><label class="switch"><input type="checkbox" data-k="${key}" ${S.settings[key] ? "checked" : ""}><i></i></label></div>`;
function vSettings() {
  const s = S.settings, e = S.engine;
  const cpus = S.info.cpus || 8;
  const thr = [`<option value="0">${t("auto")} (${cpus})</option>`].concat(Array.from({ length: Math.max(cpus, 16) }, (_, i) => `<option value="${i + 1}" ${s.threads === i + 1 ? "selected" : ""}>${i + 1}</option>`)).join("");
  const sizes = [100, 256, 512, 1024, 2048, 4000].map((m) => `<option value="${m}" ${s.max_file_mb === m ? "selected" : ""}>${m >= 1024 ? (m / 1024).toLocaleString(loc(), { maximumFractionDigits: 1 }) + " GB" : m + " MB"}</option>`).join("");
  return `<div class="head"><div><h1>${t("s_title")}</h1></div></div>
  <div class="grid g2" style="align-items:start">
  <div>
    <div class="card sgroup"><h2>${ic("sparkles")}${t("s_general")}</h2>
      <div class="srow"><div class="l"><b>${t("s_language")}</b></div><select class="input" data-k="language"><option value="de" ${s.language === "de" ? "selected" : ""}>Deutsch</option><option value="en" ${s.language === "en" ? "selected" : ""}>English</option></select></div>
      <div class="srow" style="display:block"><div class="l" style="margin-bottom:10px"><b>${t("s_theme")}</b></div><div class="themes">${THEMES.map(([k, n, bg, a, b]) => `<button class="sw ${s.theme === k ? "on" : ""}" data-theme-k="${k}"><div style="background:linear-gradient(135deg, ${bg} 45%, ${a} 46%, ${b})"></div><span>${n}</span></button>`).join("")}</div></div>
      ${sw("notify_on_finish", t("s_notify"))}
      <div class="srow"><div class="l"><b>${t("s_tutorial")}</b></div><button class="btn sm" id="bTut">${ic("play")}${t("s_tutorial_btn")}</button></div>
    </div>
    <div class="card sgroup"><h2>${ic("scan")}${t("s_scan")}</h2>
      <div class="srow"><div class="l"><b>${t("s_threads")}</b><span>${t("s_threads_sub")}</span></div><select class="input" data-k="threads" data-num>${thr}</select></div>
      <div class="srow"><div class="l"><b>${t("s_maxsize")}</b><span>${t("s_maxsize_sub")}</span></div><select class="input" data-k="max_file_mb" data-num>${sizes}</select></div>
      ${sw("scan_archives", t("s_archives"), t("s_archives_sub"))}
      ${sw("heuristics", t("s_heur"), t("s_heur_sub"))}
      ${sw("detect_pua", t("s_pua"), t("s_pua_sub"))}
      ${sw("skip_unchanged", t("s_cache"), t("s_cache_sub"))}
      ${sw("follow_symlinks", t("s_symlinks"), t("s_symlinks_sub"))}
      ${sw("skip_hidden", t("s_hidden"), t("s_hidden_sub"))}
      <div class="srow"><div class="l"><b>${t("s_cache_clear")}</b></div><button class="btn sm" id="bCache">${ic("trash")}${t("s_cache_clear")}</button></div>
    </div>
  </div>
  <div>
    <div class="card sgroup"><h2>${ic("shield")}${t("s_engine")}</h2>
      <div class="kv" style="margin-bottom:12px"><span class="muted">${t("card_engine")}</span><span>${esc(e.engine_version || "–")} ${e.bundled ? '<span class="tag">AppImage</span>' : ""}</span>
        <span class="muted">${t("card_signatures")}</span><span>${esc(e.signature_version || "–")} · ${esc(sigDate(e.signature_date))} ${e.db_size ? "· " + bytes(e.db_size) : ""}</span>
        <span class="muted">Status</span><span>${esc(t("st_" + engineView().key))}</span></div>
      <div style="display:flex;gap:8px;flex-wrap:wrap;margin-bottom:6px">
        <button class="btn sm primary" id="bSig" ${e.updating || !e.available ? "disabled" : ""}>${ic("refresh")}${t(e.db_present ? "btn_update_sigs" : "btn_download_sigs")}</button>
        <button class="btn sm" id="bEng" ${!e.db_present ? "disabled" : ""}>${ic("cpu")}${t("s_restart_engine")}</button></div>
      ${sw("auto_update_signatures", t("s_autoupdate"), t("s_autoupdate_sub"))}
      ${sw("preload_engine", t("s_preload"), t("s_preload_sub"))}
      <div style="margin-top:10px"><div class="muted" style="font-size:12px;margin-bottom:6px">${t("s_log")}</div><div class="log mono" id="flog">${esc(S.freshLog.join("\n"))}</div></div>
    </div>
    <div class="card sgroup"><h2>${ic("folder")}${t("s_excl")}</h2><div class="muted" style="font-size:12.5px;margin-bottom:10px">${t("s_excl_sub")}</div>
      <div class="list" id="exList">${s.exclusions.map((x, i) => `<div class="li"><span class="mono">${esc(x)}</span><button class="icon-btn bad" data-ex="${i}">${ic("x")}</button></div>`).join("")}</div>
      <div style="display:flex;gap:8px;margin-top:10px"><input class="input mono" id="exIn" placeholder="${t("s_excl_input")}" style="flex:1"><button class="btn sm" id="exAdd">${ic("plus")}</button><button class="btn sm" id="exPick">${ic("folder")}${t("s_excl_add")}</button></div>
      <div class="srow" style="margin-top:6px"><div class="l"><b>${t("s_ext")}</b><span>${t("s_ext_sub")}</span></div><input class="input mono" id="extIn" style="width:180px" value="${esc(s.excluded_extensions.join(", "))}"></div>
    </div>
    <div class="card sgroup"><h2>${ic("eye-off")}${t("s_wl")}</h2><div class="muted" style="font-size:12.5px;margin-bottom:10px">${t("s_wl_sub")}</div>
      <div class="list">${s.whitelist.length ? s.whitelist.map((w) => `<div class="li"><span class="mono" title="${esc(w.sha256)}">${esc(w.name)} <span class="dim">· ${esc(w.signature)}</span></span><button class="icon-btn bad" data-wl="${esc(w.sha256)}">${ic("x")}</button></div>`).join("") : `<div class="dim">${t("s_wl_empty")}</div>`}</div>
    </div>
    <div class="card sgroup"><h2>${ic("download")}${t("s_updates")}</h2>
      ${sw("check_app_updates", t("s_check_updates"))}
      <div class="srow"><div class="l"><b>${esc(S.info.label || "")}</b><span>${t("s_data")}: <span class="mono">${esc(S.info.data_dir || "")}</span></span></div><button class="btn sm" id="bUpd">${ic("refresh")}${t("s_check_now")}</button></div>
    </div>
  </div></div>`;
}
bind.settings = () => {
  document.querySelectorAll("[data-k]").forEach((el) => (el.onchange = () => {
    const k = el.dataset.k;
    const v = el.type === "checkbox" ? el.checked : el.dataset.num !== undefined ? Number(el.value) : el.value;
    saveSettings({ [k]: v }).then(() => { if (k === "language") render(); });
  }));
  document.querySelectorAll("[data-theme-k]").forEach((b) => (b.onclick = () => saveSettings({ theme: b.dataset.themeK }, true).then(render)));
  $("#bTut").onclick = () => onboarding(0);
  $("#bCache").onclick = async () => { await invoke("clear_scan_cache"); toast(t("cache_cleared")); };
  $("#bSig").onclick = updateSigs;
  $("#bEng").onclick = () => { invoke("engine_start"); toast(t("engine_restarting"), "info"); };
  $("#bUpd").onclick = () => checkUpdate(true);
  const addEx = (p) => { p = p.trim(); if (!p || S.settings.exclusions.includes(p)) return; saveSettings({ exclusions: [...S.settings.exclusions, p] }).then(render); };
  $("#exAdd").onclick = () => addEx($("#exIn").value);
  $("#exIn").onkeydown = (e) => { if (e.key === "Enter") addEx(e.target.value); };
  $("#exPick").onclick = async () => { const p = await invoke("pick_folders"); if (p && p.length) saveSettings({ exclusions: [...new Set([...S.settings.exclusions, ...p])] }).then(render); };
  document.querySelectorAll("[data-ex]").forEach((b) => (b.onclick = () => saveSettings({ exclusions: S.settings.exclusions.filter((_, i) => i !== Number(b.dataset.ex)) }).then(render)));
  $("#extIn").onchange = (e) => saveSettings({ excluded_extensions: e.target.value.split(",").map((x) => x.trim().replace(/^\./, "").toLowerCase()).filter(Boolean) });
  document.querySelectorAll("[data-wl]").forEach((b) => (b.onclick = async () => { S.settings = await invoke("whitelist_remove", { sha256: b.dataset.wl }); render(); }));
  const lg = $("#flog"); if (lg) lg.scrollTop = lg.scrollHeight;
};

// ---------------- about
function vAbout() {
  return `<div class="head"><div><h1>${t("about_title")}</h1><div class="sub">${esc(S.info.label || "")}</div></div></div>
  <div class="card hero"><img src="logo.svg" style="width:96px;height:96px;border-radius:24px" alt=""><div><h2>LakuenFileScanner</h2><p>${t("about_text")}</p>
    <div class="row"><button class="btn" id="bGh">${ic("github")}${t("github")}</button></div><p class="muted" style="margin:14px 0 0">${t("credits")}</p></div></div>
  <div class="grid g2" style="margin-top:14px">
    <div class="card"><h2>${t("about_shortcuts")}</h2><div class="kv">
      <span><kbd>Ctrl</kbd> + <kbd>O</kbd></span><span>${t("sc_files")}</span>
      <span><kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>O</kbd></span><span>${t("sc_folder")}</span>
      <span><kbd>Ctrl</kbd> + <kbd>Q</kbd></span><span>${t("sc_quick")}</span>
      <span><kbd>Esc</kbd></span><span>${t("sc_cancel")}</span>
      <span><kbd>Ctrl</kbd> + <kbd>,</kbd></span><span>${t("sc_settings")}</span>
      <span><kbd>Ctrl</kbd> + <kbd>1</kbd>–<kbd>7</kbd></span><span>${NAV.map(([k]) => t("nav_" + k)).join(" · ")}</span></div></div>
    <div class="card"><h2>${t("about_licenses")}</h2><p class="muted" style="margin:0">${t("about_lic_text")}</p></div>
  </div>`;
}
bind.about = () => { $("#bGh").onclick = () => invoke("open_url", { url: `https://github.com/${S.info.repo}` }); };

// ------------------------------------------------------------------ updates
function renderBanner() {
  const u = S.update, el = $("#banner");
  if (!u || !u.available || u.dismissed) { el.innerHTML = ""; return; }
  if (S.updating) {
    const p = S.updating;
    const pct = p.total ? Math.round((p.downloaded / p.total) * 100) : 0;
    const txt = p.stage === "verify" ? t("upd_verify") : p.stage === "restart" ? t("upd_restart") : t("upd_downloading", { p: `${pct}% · ${bytes(p.downloaded)}` });
    el.innerHTML = `<div class="banner">${ic("download")}<span class="grow">${esc(txt)}</span><div class="bar"><i style="width:${pct}%"></i></div></div>`;
    return;
  }
  el.innerHTML = `<div class="banner">${ic("sparkles")}<span class="grow"><b>${esc(t("upd_banner", { v: u.latest_label }))}</b></span>
    <button class="btn sm ghost" id="uNotes">${t("upd_notes")}</button><button class="btn sm ghost" id="uLater">${t("upd_later")}</button>
    <button class="btn sm primary" id="uGo">${ic("download")}${t("upd_btn")}</button></div>`;
  $("#uNotes").onclick = () => { const m = modal(`<h2>${esc(t("upd_banner", { v: u.latest_label }))}</h2><div class="notes">${esc(u.notes || "–")}</div><div class="foot"><button class="btn" id="mClose">${t("close")}</button></div>`, { wide: true }); m.el.querySelector("#mClose").onclick = m.close; };
  $("#uLater").onclick = () => { S.update.dismissed = true; renderBanner(); };
  $("#uGo").onclick = doUpdate;
}
async function checkUpdate(manual) {
  try {
    const u = await invoke("check_update");
    S.update = u; renderBanner();
    if (manual && !u.available) toast(t("up_to_date", { v: u.current_label }));
  } catch (e) { if (manual) toast(t("update_failed", { msg: errText(e) }), "bad"); }
}
async function doUpdate() {
  if (!S.update.can_self_update) { toast(t("upd_manual"), "info"); invoke("open_url", { url: S.update.page_url }); return; }
  if (S.running) return toast(t("err_busy"), "info");
  S.updating = { downloaded: 0, total: S.update.size, stage: "download" }; renderBanner();
  try { await invoke("apply_update"); } catch (e) { S.updating = null; renderBanner(); toast(t("update_failed", { msg: errText(e) }), "bad"); }
}

// ------------------------------------------------------------------ onboarding
function onboarding(step = 0) {
  const steps = [
    { t: "ob1_t", s: "ob1_s", img: true },
    { t: "ob2_t", s: "ob2_s", sigs: true },
    { t: "ob3_t", s: "ob3_s" },
  ];
  const st = steps[step], e = S.engine;
  const sigReady = e.db_present && !e.updating;
  const body = `<div class="steps">${steps.map((_, i) => `<i class="${i <= step ? "on" : ""}"></i>`).join("")}</div>
    ${st.img ? `<img class="ob-hero" src="logo.svg" alt="">` : ""}
    <h2>${t(st.t)}</h2><p>${t(st.s)}</p>
    ${st.sigs ? (sigReady ? `<div class="pill" style="color:var(--ok)">${ic("check")}${t("ob2_done")}</div>`
      : `<button class="btn primary" id="obSig" ${e.updating || !e.available ? "disabled" : ""}>${ic("download")}${t("btn_download_sigs")}</button><div class="log mono" id="obLog" style="margin-top:12px;height:120px">${esc(S.freshLog.join("\n"))}</div>`) : ""}
    <div class="foot">${step === 0 ? `<button class="btn ghost" id="obSkip">${t("ob_skip")}</button>` : `<button class="btn ghost" id="obBack">${t("ob_back")}</button>`}<span class="grow"></span>
      <button class="btn primary" id="obNext">${step === steps.length - 1 ? t("ob_finish") : t("ob_next")}</button></div>`;
  const finish = () => { $("#modal").innerHTML = ""; S.onb = null; if (!S.settings.onboarding_done) saveSettings({ onboarding_done: true }, true); };
  const m = modal(body, { wide: true, onClose: finish });
  S.onb = step;
  const q = (id) => m.el.querySelector(id);
  if (q("#obSkip")) q("#obSkip").onclick = finish;
  if (q("#obBack")) q("#obBack").onclick = () => onboarding(step - 1);
  q("#obNext").onclick = () => (step === steps.length - 1 ? finish() : onboarding(step + 1));
  if (q("#obSig")) q("#obSig").onclick = () => { updateSigs(); onboarding(step); };
  const lg = q("#obLog"); if (lg) lg.scrollTop = lg.scrollHeight;
}

// ------------------------------------------------------------------ events
function appendLog(line) {
  S.freshLog.push(line); if (S.freshLog.length > 400) S.freshLog.shift();
  for (const id of ["#flog", "#obLog"]) { const el = $(id); if (el) { el.textContent = S.freshLog.join("\n"); el.scrollTop = el.scrollHeight; } }
}
listen("engine-status", (e) => { S.engine = e; renderPill(); if (S.view === "dashboard" || S.view === "settings") render(); if (S.onb != null) onboarding(S.onb); });
listen("freshclam-log", appendLog);
listen("signatures-updated", async (err) => {
  S.engine = await invoke("engine_status"); S.settings = await invoke("get_settings");
  err ? toast(t("sig_failed", { msg: err }), "bad") : toast(t("sig_updated"));
  renderPill(); if (S.view === "dashboard" || S.view === "settings") render(); if (S.onb != null) onboarding(S.onb);
});
listen("scan-started", () => { S.running = true; renderNav(); });
listen("scan-progress", (p) => { S.progress = p; if (S.view === "scan") updateScanView(); });
listen("scan-finding", (f) => {
  S.live.push(f);
  if (S.view === "scan" && $("#liveBody")) { const e = $("#liveEmpty"); if (e) e.remove(); $("#liveBody").insertAdjacentHTML("beforeend", liveRow(f)); }
});
listen("scan-done", (s) => {
  S.running = false; S.last = s; S.history.unshift(s);
  if (S.view === "scan") { updateScanView(); } else render();
  renderNav();
  if (s.findings.length) toast(t("findings_short", { n: s.findings.length }), "bad");
});
listen("update-progress", (p) => { S.updating = p; renderBanner(); });
listen("tauri://drag-enter", () => $("#dropOverlay").classList.add("on"));
listen("tauri://drag-leave", () => $("#dropOverlay").classList.remove("on"));
listen("tauri://drag-drop", (p) => { $("#dropOverlay").classList.remove("on"); const paths = p?.paths || []; if (paths.length) startScan(paths); });

document.addEventListener("keydown", (e) => {
  if ($("#modal").innerHTML && e.key === "Escape") { $("#modal .overlay")?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true })); return; }
  if (e.target.matches("input, select, textarea")) return;
  const c = e.ctrlKey || e.metaKey;
  if (c && e.key.toLowerCase() === "o") { e.preventDefault(); pickAndScan(e.shiftKey ? "folder" : "files"); }
  else if (c && e.key.toLowerCase() === "q") { e.preventDefault(); quickScan(); }
  else if (c && e.key === ",") { e.preventDefault(); go("settings"); }
  else if (c && /^[1-7]$/.test(e.key)) { e.preventDefault(); go(NAV[Number(e.key) - 1][0]); }
  else if (e.key === "Escape" && S.running) cancelScan();
});
document.addEventListener("contextmenu", (e) => { if (!e.target.matches("input, textarea")) e.preventDefault(); });

// ------------------------------------------------------------------ init
(async () => {
  const [info, settings, engine, st, history, quarantine] = await Promise.all([
    invoke("app_info"), invoke("get_settings"), invoke("engine_status"), invoke("scan_state"), invoke("history_list"), invoke("quarantine_list"),
  ]);
  Object.assign(S, { info, settings, engine, history, quarantine, running: st.running, last: st.last, progress: st.running ? st.progress : null });
  applyStatic();
  render();
  if (!settings.onboarding_done) onboarding(0);
  if (settings.check_app_updates) setTimeout(() => checkUpdate(false), 1500);
  const targets = await invoke("startup_targets");
  if (targets.length) startScan(targets);
})();
}
