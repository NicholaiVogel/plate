#!/usr/bin/env bash
# Render Plate's vector mark and build every native macOS icon size.
set -euo pipefail
cd "$(dirname "$0")/.."
[[ "$(uname -s)" == Darwin ]] || { echo "This script requires macOS" >&2; exit 1; }
renderer="$(command -v rsvg-convert || true)"
if [[ -z "$renderer" && -x target/macos-deps/bin/rsvg-convert ]]; then
  renderer="$PWD/target/macos-deps/bin/rsvg-convert"
fi
[[ -n "$renderer" ]] || { echo "Install librsvg (brew install librsvg) to regenerate the icon" >&2; exit 1; }
"$renderer" --width 1024 --height 1024 packaging/plate.svg --output packaging/plate-macos.png
iconset="$PWD/target/Plate.iconset"
mkdir -p "$iconset"
for item in '16 16x16' '32 16x16@2x' '32 32x32' '64 32x32@2x' \
  '128 128x128' '256 128x128@2x' '256 256x256' '512 256x256@2x' \
  '512 512x512' '1024 512x512@2x'; do
  read -r pixels name <<< "$item"
  /usr/bin/sips -z "$pixels" "$pixels" packaging/plate-macos.png \
    --out "$iconset/icon_$name.png" >/dev/null
done
/usr/bin/iconutil -c icns "$iconset" -o packaging/Plate.icns
echo "Built packaging/Plate.icns"
