#!/bin/bash
# Cloud sessions start from a fresh clone with no node_modules, so `bun test`
# and `bun run lint` fail until dependencies are installed.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "$CLAUDE_PROJECT_DIR"
bun install
