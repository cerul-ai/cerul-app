#!/usr/bin/env bash

prepare_corepack_pnpm_path() {
  local root="${1:?missing repo root}"
  local dry_run="${2:-0}"

  if [ "${CERUL_COREPACK_PNPM_PREPARED:-0}" = "1" ]; then
    return
  fi

  if ! command -v corepack >/dev/null 2>&1; then
    return
  fi

  if [ "$dry_run" = "1" ]; then
    echo "+ prepare corepack pnpm shim"
    return
  fi

  local shim_dir="$root/.tmp/corepack-pnpm"
  mkdir -p "$shim_dir"
  cat > "$shim_dir/pnpm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exec corepack pnpm "$@"
EOF
  chmod +x "$shim_dir/pnpm"

  export PATH="$shim_dir:$PATH"
  export CERUL_COREPACK_PNPM_PREPARED=1

  local expected_pnpm=""
  if command -v node >/dev/null 2>&1 && [ -f "$root/package.json" ]; then
    expected_pnpm="$(
      cd "$root" &&
        node -p "const pm=require('./package.json').packageManager||''; const m=pm.match(/^pnpm@(.+)$/); m ? m[1] : ''"
    )"
  fi

  if [ -n "$expected_pnpm" ]; then
    local actual_pnpm
    actual_pnpm="$(pnpm --version 2>/dev/null || true)"
    if [ "$actual_pnpm" != "$expected_pnpm" ]; then
      echo "Expected pnpm $expected_pnpm from packageManager, but resolved pnpm $actual_pnpm." >&2
      exit 2
    fi
  fi
}
