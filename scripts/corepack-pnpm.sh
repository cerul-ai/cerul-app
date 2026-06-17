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

  local expected_pnpm=""
  if command -v node >/dev/null 2>&1 && [ -f "$root/package.json" ]; then
    expected_pnpm="$(
      cd "$root" &&
        node -p "const pm=require('./package.json').packageManager||''; const m=pm.match(/^pnpm@(.+)$/); m ? m[1] : ''"
    )"
  fi

  local corepack_pnpm_spec="pnpm"
  local pnpm_cjs=""
  if [ -n "$expected_pnpm" ]; then
    corepack_pnpm_spec="pnpm@$expected_pnpm"
    corepack "$corepack_pnpm_spec" --version >/dev/null 2>&1 || true
    for candidate in \
      "$HOME/.cache/node/corepack/v1/pnpm/$expected_pnpm/bin/pnpm.cjs" \
      "$HOME/.cache/node/corepack/pnpm/$expected_pnpm/bin/pnpm.cjs"
    do
      if [ -f "$candidate" ]; then
        pnpm_cjs="$candidate"
        break
      fi
    done
  fi

  local shim_dir="$root/.tmp/corepack-pnpm"
  mkdir -p "$shim_dir"
  if [ -n "$pnpm_cjs" ]; then
    cat > "$shim_dir/pnpm" <<EOF
#!/usr/bin/env bash
set -euo pipefail
exec node "$pnpm_cjs" "\$@"
EOF
  else
    cat > "$shim_dir/pnpm" <<EOF
#!/usr/bin/env bash
set -euo pipefail
exec corepack "$corepack_pnpm_spec" "\$@"
EOF
  fi
  chmod +x "$shim_dir/pnpm"

  export PATH="$shim_dir:$PATH"
  export CERUL_COREPACK_PNPM_PREPARED=1

  if [ -n "$expected_pnpm" ]; then
    local actual_pnpm
    actual_pnpm="$(pnpm --version 2>/dev/null || true)"
    if [ "$actual_pnpm" != "$expected_pnpm" ]; then
      echo "Expected pnpm $expected_pnpm from packageManager, but resolved pnpm $actual_pnpm." >&2
      exit 2
    fi
  fi
}
