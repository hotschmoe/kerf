#!/usr/bin/env bash
# Idempotent setup of Kerf eval tooling: python venv (tools/.venv) + node deps (tools/node_modules).
set -euo pipefail
cd "$(dirname "$0")"

PY_PKGS=(ezdxf matplotlib pypdf pypdfium2 pdfplumber brotli pillow numpy)

if [ ! -x .venv/bin/python ]; then
  if command -v uv >/dev/null 2>&1; then
    uv venv .venv
  elif python3 -m venv .venv 2>/dev/null; then :
  else
    rm -rf .venv
    echo "No uv and python3 -m venv failed (install python3-venv or uv)" >&2; exit 1
  fi
fi
if command -v uv >/dev/null 2>&1; then
  uv pip install --python .venv/bin/python --quiet "${PY_PKGS[@]}"
else
  .venv/bin/python -m pip install --quiet "${PY_PKGS[@]}"
fi

[ -f package.json ] || echo '{"name":"kerf-tools","private":true,"type":"module"}' > package.json
if [ ! -d node_modules/puppeteer-core ]; then
  PUPPETEER_SKIP_DOWNLOAD=1 npm install --no-audit --no-fund --silent puppeteer-core
fi
echo "setup ok: $(.venv/bin/python -c 'import ezdxf,pypdfium2;print("ezdxf",ezdxf.__version__,"pdfium",pypdfium2.version.PYPDFIUM_INFO)')"
