#!/usr/bin/env bash
# End-to-end test of the bundled engine with the same config the app writes:
# freshclam downloads signatures, clamd loads them, EICAR must be FOUND.
set -euo pipefail
cd "$GITHUB_WORKSPACE/dist"
rm -rf squashfs-root
./LakuenFileScanner-x86_64.AppImage --appimage-extract >/dev/null
D="$PWD/squashfs-root/usr/lib/lakuen-clamav"
DB=$(mktemp -d); RUN=$(mktemp -d); mkdir -p "$RUN/tmp"
CERTS=""; [ -d "$D/certs" ] && CERTS="CVDCertsDirectory $D/certs"
ENV=(env -i PATH=/usr/bin:/bin HOME="$HOME" LD_LIBRARY_PATH="$D/lib" CURL_CA_BUNDLE=/etc/ssl/certs/ca-certificates.crt SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt)

cat > "$RUN/freshclam.conf" <<EOF
DatabaseDirectory $DB
DatabaseMirror database.clamav.net
DNSDatabaseInfo current.cvd.clamav.net
ConnectTimeout 30
ReceiveTimeout 0
TestDatabases no
Bytecode yes
LogTime no
$CERTS
EOF
"${ENV[@]}" "$D/bin/freshclam" --config-file="$RUN/freshclam.conf" --stdout | tail -15
ls -la "$DB"

cat > "$RUN/clamd.conf" <<EOF
LocalSocket $RUN/clamd.sock
LocalSocketMode 600
FixStaleSocket yes
Foreground yes
DatabaseDirectory $DB
TemporaryDirectory $RUN/tmp
PidFile $RUN/clamd.pid
LogFile $RUN/clamd.log
LogFileMaxSize 5M
LogTime yes
MaxThreads 4
MaxQueue 100
MaxConnectionQueueLength 256
ReadTimeout 600
CommandReadTimeout 30
IdleTimeout 60
SelfCheck 3600
ConcurrentDatabaseReload no
ExitOnOOM no
MaxFileSize 1024M
MaxScanSize 2048M
MaxRecursion 17
MaxFiles 15000
ScanArchive yes
DetectPUA no
AlertExceedsMax no
Bytecode yes
$CERTS
EOF
"${ENV[@]}" "$D/bin/clamd" --foreground --config-file="$RUN/clamd.conf" &
PID=$!

python3 -I - "$RUN" <<'PY'
import socket, sys, time, os, zipfile
run = sys.argv[1]
sock = os.path.join(run, "clamd.sock")
def cmd(c):
    s = socket.socket(socket.AF_UNIX); s.settimeout(120); s.connect(sock)
    s.sendall(b"z" + c + b"\0"); out = b""
    while True:
        d = s.recv(65536)
        if not d: break
        out += d
    return out.rstrip(b"\0\n").decode()
t0 = time.time()
while True:
    try:
        if cmd(b"PING") == "PONG": break
    except OSError: pass
    if time.time() - t0 > 300: sys.exit("clamd did not start")
    time.sleep(1)
print(f"clamd ready after {time.time()-t0:.1f}s:", cmd(b"VERSION"))
e = bytes(b ^ 0x5A for b in [2,111,21,123,10,127,26,27,10,1,110,6,10,0,2,111,110,114,10,4,115,109,25,25,115,109,39,126,31,19,25,27,8,119,9,14,27,20,30,27,8,30,119,27,20,14,19,12,19,8,15,9,119,14,31,9,14,119,28,19,22,31,123,126,18,113,18,112])
f = os.path.join(run, "eicar.com"); open(f, "wb").write(e)
z = os.path.join(run, "nested.zip")
with zipfile.ZipFile(z, "w") as zf: zf.writestr("docs/readme.com", e)
c = os.path.join(run, "clean.txt"); open(c, "w").write("hello world\n" * 1000)
for path, want in ((f, "FOUND"), (z, "FOUND"), (c, "OK")):
    r = cmd(b"SCAN " + path.encode())
    print(r)
    if not r.endswith(want): sys.exit(f"unexpected: {r}")
print("::notice title=Engine E2E::EICAR + EICAR-in-ZIP detected, clean file OK (clamd ready in %.0fs)" % (time.time() - t0))
PY
kill $PID || true
wait $PID 2>/dev/null || true

# ---- 2) the real app: start headless, it must launch its own clamd
export HOME_T=$(mktemp -d) XRT=$(mktemp -d)
chmod 700 "$XRT"
mkdir -p "$HOME_T/.local/share/lakuenfilescanner/signatures"
cp "$DB"/* "$HOME_T/.local/share/lakuenfilescanner/signatures/"
HOME="$HOME_T" XDG_RUNTIME_DIR="$XRT" APPIMAGE_EXTRACT_AND_RUN=1 LAKUEN_NO_UPDATE_CHECK=1 \
  xvfb-run -a timeout 60 ./LakuenFileScanner-x86_64.AppImage > "$RUN/app.log" 2>&1 &
APP=$!
RC=0
python3 -I - "$XRT/lakuenfilescanner/clamd.sock" <<'PY' || RC=$?
import socket, sys, time, os
sock = sys.argv[1]
def cmd(c):
    s = socket.socket(socket.AF_UNIX); s.settimeout(60); s.connect(sock)
    s.sendall(b"z" + c + b"\0"); out = b""
    while True:
        d = s.recv(65536)
        if not d: break
        out += d
    return out.rstrip(b"\0\n").decode()
t0 = time.time()
while True:
    try:
        if cmd(b"PING") == "PONG": break
    except OSError: pass
    if time.time() - t0 > 50: sys.exit("app did not start its clamd")
    time.sleep(1)
e = bytes(b ^ 0x5A for b in [2,111,21,123,10,127,26,27,10,1,110,6,10,0,2,111,110,114,10,4,115,109,25,25,115,109,39,126,31,19,25,27,8,119,9,14,27,20,30,27,8,30,119,27,20,14,19,12,19,8,15,9,119,14,31,9,14,119,28,19,22,31,123,126,18,113,18,112])
f = "/tmp/lakuen-e2e-eicar.com"; open(f, "wb").write(e)
r = cmd(b"SCAN " + f.encode()); print(r)
if not r.endswith("FOUND"): sys.exit("app clamd did not detect EICAR")
print("::notice title=App E2E::App started headless, launched its own clamd (%.0fs), EICAR detected" % (time.time() - t0))
PY
kill $APP 2>/dev/null || true
echo "--- app log"; tail -20 "$RUN/app.log" || true
echo "--- clamd conf written by the app"; cat "$XRT/lakuenfilescanner/clamd.conf" || true
rm -rf squashfs-root
exit $RC
