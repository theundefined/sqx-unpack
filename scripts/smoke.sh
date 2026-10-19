#!/usr/bin/env bash
# Smoke-test a built wheel: install into a fresh venv and run the sqx binary.
# Usage: scripts/smoke.sh <wheel_dir> [synthetic.sqx]
set -euo pipefail

WHEEL_DIR="${1:?usage: smoke.sh <wheel_dir> [sqx_file]}"
SAMPLE="${2:-tests/data/synthetic.sqx}"

PY=python3
command -v "$PY" >/dev/null 2>&1 || PY=python

VENV="$(mktemp -d)/venv"
"$PY" -m venv "$VENV"
if [ -x "$VENV/Scripts/python.exe" ]; then
  VPY="$VENV/Scripts/python.exe"; SQX="$VENV/Scripts/sqx.exe"
else
  VPY="$VENV/bin/python"; SQX="$VENV/bin/sqx"
fi

"$VPY" -m pip install --no-index --find-links "$WHEEL_DIR" sqx-unpack
[ -x "$SQX" ] || { echo "sqx not found at $SQX" >&2; exit 1; }

echo "== sqx --help"
"$SQX" --help

if [ -f "$SAMPLE" ]; then
  echo "== sqx t $SAMPLE"
  "$SQX" t "$SAMPLE"
else
  echo "(no $SAMPLE, skipping archive test)"
fi
echo "smoke OK"
