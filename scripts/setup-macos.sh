#!/usr/bin/env bash
# Install native dependencies locally when system packages are unavailable.
set -euo pipefail
cd "$(dirname "$0")/.."
[[ "$(uname -s)" == Darwin ]] || { echo "This script requires macOS" >&2; exit 1; }

deps="$PWD/target/macos-deps"
export PKG_CONFIG_PATH="$deps/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
if command -v pkg-config >/dev/null && pkg-config --exists 'OpenColorIO >= 2.2' 'OpenImageIO >= 2.4' libavcodec; then
  echo "Native dependencies available"
  exit 0
fi

case "$(uname -m)" in
  x86_64) platform=osx-64 ;;
  arm64) platform=osx-arm64 ;;
  *) echo "Unsupported Mac architecture" >&2; exit 1 ;;
esac
manager="$PWD/target/micromamba/bin/micromamba"
if [[ ! -x "$manager" ]]; then
  mkdir -p target/micromamba
  curl -fLsS "https://micro.mamba.pm/api/micromamba/$platform/latest" |
    tar -xj -C target/micromamba bin/micromamba
fi
"$manager" create -y -p "$deps" -r "$PWD/target/mamba-root" -c conda-forge \
  'opencolorio=2.5' 'openimageio=3.1' 'ffmpeg>=8,<9' pkg-config
echo "Dependencies installed in $deps"
