## LakuenFileScanner v0.1.0 – erste Version / first release

**Deutsch**

- Scannt Dateien, ganze Ordner und alle Unterordner – auswählen oder per Drag & Drop
- ClamAV-Engine (1.5) direkt in der AppImage: Signaturen werden einmal in den RAM geladen, Scan parallel auf allen CPU-Kernen
- Lakuen-Regeln: Doppel-Endungen, RTL-Dateinamen, Reverse-Shells, Miner, verschleierte Skripte, versteckte Programme in /tmp
- Funde: Quarantäne (wiederherstellbar), Löschen oder Ignorieren – nur nach deiner Bestätigung
- Live-Fortschritt mit Tempo, Restzeit, Pause und Abbrechen
- Unveränderte Dateien werden bei gleicher Signatur-Version übersprungen
- Schnellscan, Selbsttest (EICAR), Verlauf, Berichte (HTML/JSON/TXT)
- Signaturen aktualisieren sich automatisch einmal täglich
- Auto-Update der App direkt aus der App (mit SHA-256-Prüfung)
- 6 Designs, Deutsch/Englisch, Tastenkürzel, Benachrichtigungen, Einführung beim ersten Start

**English**

- Scans files, whole folders and all subfolders – pick them or drag & drop
- ClamAV engine (1.5) inside the AppImage: signatures loaded into RAM once, scanning in parallel on all CPU cores
- Lakuen rules: double extensions, RTL file names, reverse shells, miners, obfuscated scripts, hidden binaries in /tmp
- Findings: quarantine (restorable), delete or ignore – only after you confirm
- Live progress with speed, ETA, pause and cancel
- Unchanged files are skipped with the same signature version
- Quick scan, self test (EICAR), history, reports (HTML/JSON/TXT)
- Signatures update automatically once a day
- In-app auto update (with SHA-256 verification)
- 6 themes, German/English, keyboard shortcuts, notifications, onboarding on first start

**Installation:** download `LakuenFileScanner-x86_64.AppImage`, make it executable (`chmod +x`), start it.

*erstellt von Dominic Lesina x Claude*
