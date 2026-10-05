#!/bin/bash
# Cloud sessions start from a fresh clone with no node_modules, so `bun test`
# and `bun run lint` fail until dependencies are installed. `cargo test` also
# needs the system libraries Tauri links on Linux (WebKitGTK and friends) and
# an ffmpeg on PATH for the converter and frame-extraction tests; the macOS app
# bundles its own ffmpeg, so this is only for the Linux container.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "$CLAUDE_PROJECT_DIR"
bun install

if ! pkg-config --exists webkit2gtk-4.1 || ! command -v ffmpeg > /dev/null; then
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -qq
  apt-get install -y -qq \
    libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev \
    libxdo-dev libssl-dev libxcb1-dev libxrandr-dev libdbus-1-dev \
    libpipewire-0.3-dev libgbm-dev libegl-dev libwayland-dev \
    ffmpeg > /dev/null
fi
