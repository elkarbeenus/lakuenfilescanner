#!/usr/bin/env bash
# Adds the ClamAV engine (clamd + freshclam and their libraries) to the
# Tauri AppImage, then repacks it with zsync update information.
set -euo pipefail

ROOT="$GITHUB_WORKSPACE"
SRC=$(ls "$ROOT"/src-tauri/target/release/bundle/appimage/*.AppImage | head -1)
OUT="$ROOT/dist"
NAME="LakuenFileScanner-x86_64.AppImage"
mkdir -p "$OUT"

WORK=$(mktemp -d)
cd "$WORK"
cp "$SRC" in.AppImage
chmod +x in.AppImage
./in.AppImage --appimage-extract >/dev/null

D=squashfs-root/usr/lib/lakuen-clamav
mkdir -p "$D/bin" "$D/lib"
cp -L /usr/sbin/clamd "$D/bin/clamd"
cp -L "$(command -v freshclam)" "$D/bin/freshclam"

# Copy shared libraries except the glibc core (always provided by the host).
for b in "$D"/bin/*; do ldd "$b"; done | awk '/=> \//{print $3}' | sort -u | while read -r lib; do
  base=$(basename "$lib")
  case "$base" in
    libc.so*|libm.so*|libdl.so*|libpthread.so*|librt.so*|ld-linux*|libresolv.so*|libutil.so*|libnsl.so*|libanl.so*) continue ;;
  esac
  cp -L "$lib" "$D/lib/"
done
# libclamav may dlopen its unrar helper
for f in /usr/lib/x86_64-linux-gnu/libclamunrar*.so*; do
  [ -e "$f" ] && cp -L "$f" "$D/lib/" || true
done

# CVD signing certificates (ClamAV >= 1.5)
for c in /etc/clamav/certs /usr/share/clamav/certs /var/lib/clamav/certs /usr/local/etc/certs /etc/certs; do
  if [ -d "$c" ]; then cp -rL "$c" "$D/certs"; break; fi
done

# Licenses
mkdir -p "$D/share"
cp /usr/share/doc/clamav/copyright "$D/share/COPYING-clamav" 2>/dev/null || true

echo "::notice title=ClamAV bundle::$(LD_LIBRARY_PATH=$D/lib $D/bin/clamd --version) | $(ls $D/lib | wc -l) libs | certs: $(ls $D/certs 2>/dev/null | tr '\n' ' ') | size: $(du -sh $D | cut -f1)"
echo "::notice title=clamd cert option::$(clamconf -g clamd.conf 2>/dev/null | grep -i -m3 cert | tr '\n' ' ')"

echo "::notice title=Largest bundled libs::$(du -h $D/lib/* | sort -rh | head -6 | tr '\n' ' ')"

# Repack: keep the original AppImage runtime, rebuild the squashfs with the same compression.
OFF=$(./in.AppImage --appimage-offset)
head -c "$OFF" in.AppImage > runtime
COMP=$(unsquashfs -s -o "$OFF" in.AppImage | awk '/^Compression/{print $2; exit}')
COMP=${COMP:-gzip}
echo "runtime offset $OFF, compression $COMP"
mksquashfs squashfs-root fs.squashfs -root-owned -noappend -comp "$COMP" -quiet
cat runtime fs.squashfs > "$OUT/$NAME"

cd "$OUT"
chmod +x "$NAME"
sha256sum "$NAME" > "$NAME.sha256"
ls -la "$OUT"
