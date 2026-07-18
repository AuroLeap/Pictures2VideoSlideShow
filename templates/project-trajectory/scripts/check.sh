#!/usr/bin/env bash
# Thin launcher for the check harness on Linux/macOS. Prefers the project venv,
# then any Python 3 on PATH. All arguments pass straight through to check.py,
# e.g.:  ./scripts/check.sh --gate G3 --tier smoke
set -euo pipefail
cd "$(dirname "$0")/.."

if [ -x .venv/bin/python ]; then
  PY=.venv/bin/python
else
  PY=""
  for cand in python3 python; do
    if command -v "$cand" >/dev/null 2>&1; then PY="$cand"; break; fi
  done
  [ -n "$PY" ] || { echo "ERROR: Python 3 not found. Run ./scripts/setup.sh first." >&2; exit 1; }
fi

exec "$PY" scripts/check.py "$@"
