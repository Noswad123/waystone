#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
bin_dir="${WAYSTONE_BIN_DIR:-$HOME/.local/bin}"
zsh_completion_dir="${WAYSTONE_ZSH_COMPLETION_DIR:-$HOME/.local/share/zsh/site-functions}"
bash_completion_dir="${WAYSTONE_BASH_COMPLETION_DIR:-$HOME/.local/share/bash-completion/completions}"
install_completions="${WAYSTONE_INSTALL_COMPLETIONS:-1}"

cargo build --release --manifest-path "$repo_root/Cargo.toml"

mkdir -p "$bin_dir"
install -m 0755 "$repo_root/target/release/waystone" "$bin_dir/waystone"
printf 'installed waystone -> %s/waystone\n' "$bin_dir"

if [[ "$install_completions" != "0" ]]; then
  mkdir -p "$zsh_completion_dir" "$bash_completion_dir"
  install -m 0644 "$repo_root/completions/zsh/_waystone" "$zsh_completion_dir/_waystone"
  install -m 0644 "$repo_root/completions/bash/waystone" "$bash_completion_dir/waystone"
  printf 'installed zsh completion -> %s/_waystone\n' "$zsh_completion_dir"
  printf 'installed bash completion -> %s/waystone\n' "$bash_completion_dir"
fi
