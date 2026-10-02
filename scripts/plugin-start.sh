#!/usr/bin/env bash
# Run by Service.qml. Picks the om binary, makes sure it is in place, then
# becomes the daemon. Nothing is compiled.
#
# The binary is OMAESTRO_BIN when set; else `om` on PATH (a build you
# installed yourself, `make install-systemd`); else the release binary, downloaded
# once into ~/.local/share/omaestro with its SHA256 checked against
# release.sha256 next to this script's parent.

set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$here/manifest.json" | head -1)"
arch="$(uname -m)"
bin_dir="$HOME/.local/share/omaestro"
url="https://github.com/iluxav/omaestro/releases/download/v$version/om-$arch"

download=0
if [[ -n "${OMAESTRO_BIN:-}" ]]; then
  bin="$OMAESTRO_BIN"
elif on_path="$(command -v om 2>/dev/null)" && [[ "$(readlink -f "$on_path")" != "$bin_dir/"* ]]; then
  # A build you installed yourself (`cargo install`, `make install-systemd`).
  bin="$on_path"
else
  # Ours: downloaded once per version, and linked as ~/.local/bin/om below.
  bin="$bin_dir/om-$version-$arch"
  download=1
fi

# Another instance (the systemd unit, say) already answers: leave it be.
# Exit 3 tells Service.qml to look again later, quietly; a plain exit 0 is
# the daemon stopping cleanly (`om restart`) and is followed by a fresh start.
if [[ -x "$bin" ]] && "$bin" status >/dev/null 2>&1; then
  echo "an omaestro daemon is already running; not starting another"
  exit 3
fi

if [[ "$download" == 1 ]]; then
  expected="$(awk -v a="$arch" '$2 == a { print $1 }' "$here/release.sha256" || true)"
  if [[ -z "$expected" ]]; then
    echo "no release checksum for $arch in release.sha256" >&2
    exit 2
  fi

  verify() { [[ "$(sha256sum "$1" | awk '{print $1}')" == "$expected" ]]; }

  if [[ ! -x "$bin" ]] || ! verify "$bin"; then
    mkdir -p "$bin_dir"
    tmp="$(mktemp "$bin_dir/om.XXXXXX")"
    echo "downloading omaestro $version for $arch"
    if ! curl -fsSL "$url" -o "$tmp"; then
      rm -f "$tmp"
      echo "download failed: $url" >&2
      exit 2
    fi
    if ! verify "$tmp"; then
      rm -f "$tmp"
      echo "checksum mismatch for $url; not running it" >&2
      exit 2
    fi
    chmod +x "$tmp"
    mv "$tmp" "$bin"
  fi
  # `om` in the terminal: ~/.local/bin is on Omarchy's PATH. Only a link of
  # ours is replaced; a binary you put there yourself stays.
  local_bin="$HOME/.local/bin"
  if [[ ! -e "$local_bin/om" || "$(readlink -f "$local_bin/om" 2>/dev/null)" == "$bin_dir/"* ]]; then
    mkdir -p "$local_bin" && ln -sfn "$bin" "$local_bin/om"
  fi
elif [[ ! -x "$bin" ]]; then
  echo "omaestro: $bin is not executable" >&2
  exit 2
fi

exec "$bin" daemon
