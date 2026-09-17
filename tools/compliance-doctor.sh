#!/usr/bin/env bash
# forme compliance doctor — M0 floor (prd/17, canonical §12). Blocking: exit 1 = not release-ready.
# Thin launcher for tools/compliance_doctor.py; runs from repo root with RELATIVE paths so the
# same command works under POSIX python3, real WSL, and Windows py.exe/python (Git Bash) alike.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if command -v py.exe >/dev/null 2>&1; then
  python_cmd=(py.exe -3)
elif command -v python3 >/dev/null 2>&1; then
  python_cmd=(python3)
elif command -v python >/dev/null 2>&1; then
  python_cmd=(python)
elif command -v py >/dev/null 2>&1; then
  python_cmd=(py -3)
else
  echo "compliance-doctor: FAIL"
  echo "[copy] Python 3 is required for deterministic copy detection"
  exit 1
fi

exec "${python_cmd[@]}" tools/compliance_doctor.py --root . "$@"
