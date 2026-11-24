use crate::scanner::ScanSummary;
use crate::util;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn human(b: u64) -> String {
    let u = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < u.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{:.1} {}", v, u[i])
}

pub fn render(s: &ScanSummary, format: &str) -> Result<String, String> {
    match format {
        "json" => serde_json::to_string_pretty(s).map_err(|e| e.to_string()),
        "txt" => {
            let mut o = String::new();
            o.push_str(&format!("LakuenFileScanner {} – Scan-Bericht / Scan report\n", util::version_label()));
            o.push_str(&format!("ID: {}\nStatus: {}\nEngine: {} (Signaturen {})\n", s.id, s.status, s.engine, s.signature_version));
            o.push_str(&format!("Ziele / Targets:\n"));
            for t in &s.targets {
                o.push_str(&format!("  {t}\n"));
            }
            o.push_str(&format!(
                "Dateien / Files: {}\nDaten / Data: {}\nDauer / Duration: {:.1}s\nÜbersprungen / Skipped: {}\nFehler / Errors: {}\nFunde / Findings: {}\n\n",
                s.files_scanned,
                human(s.bytes_scanned),
                s.duration_ms as f64 / 1000.0,
                s.skipped + s.cached,
                s.errors,
                s.findings.len()
            ));
            for f in &s.findings {
                o.push_str(&format!("[{}] {}  {}  ({})\n", f.severity, f.signature, f.path, f.status));
            }
            o.push_str("\nerstellt von Dominic Lesina x Claude\n");
            Ok(o)
        }
        _ => {
            let rows: String = s
                .findings
                .iter()
                .map(|f| {
                    format!(
                        "<tr><td><span class=\"sev {}\">{}</span></td><td>{}</td><td class=\"p\">{}</td><td>{}</td><td>{}</td></tr>",
                        esc(&f.severity),
                        esc(&f.severity),
                        esc(&f.signature),
                        esc(&f.path),
                        human(f.size),
                        esc(&f.status)
                    )
                })
                .collect();
            let targets: String = s.targets.iter().map(|t| format!("<li>{}</li>", esc(t))).collect();
            Ok(format!(
                r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>LakuenFileScanner – Report {id}</title>
<style>body{{font-family:system-ui,sans-serif;background:#0b1020;color:#e6e9f2;margin:40px}}h1{{margin:0 0 4px}}.muted{{color:#8a93ab}}
.grid{{display:grid;grid-template-columns:repeat(4,1fr);gap:12px;margin:24px 0}}.card{{background:#141a2e;border:1px solid #232b45;border-radius:12px;padding:14px}}
.card b{{display:block;font-size:22px}}table{{width:100%;border-collapse:collapse;background:#141a2e;border-radius:12px;overflow:hidden}}
td,th{{padding:10px 12px;border-bottom:1px solid #232b45;text-align:left;font-size:14px}}.p{{font-family:monospace;word-break:break-all}}
.sev{{padding:2px 8px;border-radius:99px;font-size:12px;font-weight:600}}.infected{{background:#ff4d6d33;color:#ff8fa3}}.pua{{background:#ffb70333;color:#ffd166}}.suspicious{{background:#7c5cff33;color:#b8a6ff}}</style></head><body>
<h1>LakuenFileScanner</h1><div class="muted">Scan-Bericht / Scan report · {ver} · ID {id} · {status}</div>
<div class="grid"><div class="card"><span class="muted">Dateien / Files</span><b>{files}</b></div><div class="card"><span class="muted">Daten / Data</span><b>{bytes}</b></div>
<div class="card"><span class="muted">Funde / Findings</span><b>{nf}</b></div><div class="card"><span class="muted">Dauer / Duration</span><b>{dur:.1}s</b></div></div>
<p class="muted">Engine: {engine} · Signaturen / Signatures: {sig} · Fehler / Errors: {errors} · Übersprungen / Skipped: {skipped}</p>
<h3>Ziele / Targets</h3><ul>{targets}</ul>
<h3>Funde / Findings</h3><table><tr><th>Stufe</th><th>Signatur</th><th>Pfad</th><th>Größe</th><th>Status</th></tr>{rows}</table>
<p class="muted" style="margin-top:30px">erstellt von Dominic Lesina x Claude</p></body></html>"#,
                id = esc(&s.id),
                ver = esc(&util::version_label()),
                status = esc(&s.status),
                files = s.files_scanned,
                bytes = human(s.bytes_scanned),
                nf = s.findings.len(),
                dur = s.duration_ms as f64 / 1000.0,
                engine = esc(&s.engine),
                sig = esc(&s.signature_version),
                errors = s.errors,
                skipped = s.skipped + s.cached,
                targets = targets,
                rows = rows
            ))
        }
    }
}
