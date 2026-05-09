#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bin_dir="${HOME}/.local/bin"
bin_path="${bin_dir}/tau"
profile="${HOME}/.zshrc"

cd "$repo_root"
cargo build --release

mkdir -p "$bin_dir"
ln -sf "${repo_root}/target/release/tau" "$bin_path"

case ":${PATH}:" in
  *":${bin_dir}:"*) ;;
  *)
    if [ -f "$profile" ] && grep -Fq 'HOME/.local/bin' "$profile"; then
      :
    else
      printf '\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$profile"
      printf 'added ~/.local/bin to %s\n' "$profile"
    fi
    ;;
esac

printf 'installed tau at %s\n' "$bin_path"
printf 'open a new shell, then run: tau\n'
