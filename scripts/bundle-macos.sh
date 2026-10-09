#!/usr/bin/env bash
# Packt die Release-Binary samt FFmpeg-Bibliotheken in ein FrameScope.app, signiert ad hoc
# und prüft, dass die App startet.
# Aufruf: scripts/bundle-macos.sh <Binary> <FFmpeg-Prefix> <Version> <Ausgabeordner>
set -euo pipefail

BIN="${1:?Binary}"; FF="${2:?FFmpeg-Prefix}"; VERSION="${3:?Version}"; OUT="${4:?Ausgabeordner}"
APP="$OUT/FrameScope.app"
FW="$APP/Contents/Frameworks"
RES="$APP/Contents/Resources"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$FW" "$RES/licenses"
cp "$BIN" "$APP/Contents/MacOS/framescope"
chmod +x "$APP/Contents/MacOS/framescope"

# FFmpeg-Bibliotheken (und deren Abhängigkeiten untereinander) ins Bundle holen und auf @rpath umstellen.
fix_deps() {
  local file="$1"
  otool -L "$file" | awk 'NR>1 {print $1}' | grep -E "^${FF}/" | while read -r dep; do
    local base; base="$(basename "$dep")"
    if [ ! -f "$FW/$base" ]; then
      cp -L "$dep" "$FW/$base"
      chmod u+w "$FW/$base"
      install_name_tool -id "@rpath/$base" "$FW/$base"
      fix_deps "$FW/$base"
    fi
    install_name_tool -change "$dep" "@rpath/$base" "$file"
  done
}
fix_deps "$APP/Contents/MacOS/framescope"
install_name_tool -add_rpath "@executable_path/../Frameworks" "$APP/Contents/MacOS/framescope" 2>/dev/null || true

# Icon aus dem 1024-px-PNG.
ICONSET="$(mktemp -d)/FrameScope.iconset"
mkdir -p "$ICONSET"
SRC="assets/icon-1024.png"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$SRC" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  sips -z "$((size * 2))" "$((size * 2))" "$SRC" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$RES/FrameScope.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>FrameScope</string>
  <key>CFBundleDisplayName</key><string>FrameScope</string>
  <key>CFBundleIdentifier</key><string>io.github.supaeasy.framescope</string>
  <key>CFBundleExecutable</key><string>framescope</string>
  <key>CFBundleIconFile</key><string>FrameScope</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.video</string>
</dict>
</plist>
PLIST

# Lizenzen ins Bundle.
cp LICENSE THIRD-PARTY-LICENSES.md "$RES/licenses/"
cp "$FF/LICENSE.txt" "$RES/licenses/FFmpeg-LICENSE.txt"
cp "$FF/FFMPEG-BUILD-INFO.txt" "$RES/licenses/"
cp assets/fonts/Inter-OFL.txt "$RES/licenses/Inter-OFL.txt"

# Ad-hoc-Signatur (auf Apple Silicon Pflicht; ersetzt keine Notarisierung).
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"

# Smoke-Test: startet nur, wenn alle Bibliotheken gefunden werden.
"$APP/Contents/MacOS/framescope" --version
echo "Bundle fertig: $APP"
otool -L "$APP/Contents/MacOS/framescope" | head -20
