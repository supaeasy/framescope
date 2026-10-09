#!/usr/bin/env bash
# Baut FFmpeg als LGPL-Shared-Build für macOS (ohne GPL-/nonfree-Teile und ohne externe Bibliotheken).
# Aufruf: scripts/build-ffmpeg-macos.sh <Zielordner> [Version]
set -euo pipefail

PREFIX="${1:?Zielordner fehlt}"
VERSION="${2:-${FFMPEG_VERSION:-9.0.2}}"
ARCH="$(uname -m)"                     # arm64 | x86_64 (nativer Build)
WORK="$(mktemp -d)"
URL="https://ffmpeg.org/releases/ffmpeg-${VERSION}.tar.xz"

echo "FFmpeg ${VERSION} (${ARCH}) -> ${PREFIX}"
curl -fsSL "$URL" -o "$WORK/ffmpeg.tar.xz"
tar -xf "$WORK/ffmpeg.tar.xz" -C "$WORK"
cd "$WORK/ffmpeg-${VERSION}"

CONFIGURE_ARGS=(
  --prefix="$PREFIX"
  --disable-gpl --disable-nonfree
  --enable-shared --disable-static --enable-pic
  --disable-programs --disable-doc --disable-debug
  --disable-avdevice --disable-avfilter --disable-network
  --disable-autodetect --enable-zlib
  --extra-cflags="-mmacosx-version-min=11.0"
  --extra-ldflags="-mmacosx-version-min=11.0"
)
./configure "${CONFIGURE_ARGS[@]}" 2>&1 | tail -n 25
make -j"$(sysctl -n hw.ncpu)"
make install

# Lizenz und Bauinfo für die Weitergabe (LGPL: Quelle und Konfiguration nennen).
cp COPYING.LGPLv2.1 "$PREFIX/LICENSE.txt"
{
  echo "FFmpeg ${VERSION}, unveränderte Quellen von ${URL}"
  echo "Gebaut für macOS ${ARCH} als LGPL-Shared-Build (ohne GPL/nonfree, ohne externe Bibliotheken)."
  echo "configure ${CONFIGURE_ARGS[*]}"
} > "$PREFIX/FFMPEG-BUILD-INFO.txt"
cat "$PREFIX/FFMPEG-BUILD-INFO.txt"
