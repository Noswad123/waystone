#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
bin_dir="${WAYSTONE_BIN_DIR:-$HOME/.local/bin}"

mkdir -p "$bin_dir"
install -m 0755 "$repo_root/bin/waystone" "$bin_dir/waystone"
printf 'installed waystone -> %s/waystone\n' "$bin_dir"
