#!/usr/bin/env bash
# One-shot dev setup for Linux/macOS. Makes a fresh clone runnable. Idempotent.
# Edit the dependency list for your project; the reference installs the tools the
# Python check harness uses. Windows: use scripts/setup.ps1.
set -euo pipefail
cd "$(dirname "$0")/.."

# Find a Python 3 interpreter.
PY=""
for cand in python3 python; do
  if command -v "$cand" >/dev/null 2>&1; then PY="$cand"; break; fi
done
[ -n "$PY" ] || { echo "ERROR: Python 3 not found on PATH." >&2; exit 1; }
echo "Using $($PY --version)"

# Create/activate a local virtualenv so installs don't touch the system Python.
if [ ! -d .venv ]; then
  echo "Creating .venv ..."
  "$PY" -m venv .venv
fi
# shellcheck disable=SC1091
. .venv/bin/activate

python -m pip install --upgrade pip
# --- Edit below for your stack -------------------------------------------------
pip install ruff pytest pytest-cov
if [ -f pyproject.toml ]; then pip install -e .
elif [ -f requirements.txt ]; then pip install -r requirements.txt
fi
# ------------------------------------------------------------------------------

echo
echo "Setup complete. Run the harness with: ./scripts/check.sh --gate G3"
echo "(check.sh uses the venv python directly; activating is optional.)"
