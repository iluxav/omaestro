#!/usr/bin/env bash
# `make install` / `make uninstall`: this checkout installed the way a user
# gets omaestro from the Omarchy marketplace, as close as a local build
# allows, so the first run can be tried for real.
#
#   install    builds the release binary and leaves it where the plugin's
#              start script downloads it to (~/.local/share/omaestro/om-<version>-<arch>);
#              stages the plugin (the working tree, uncommitted changes
#              included) as a git repository under target/ whose release.sha256
#              pins that build; then `omarchy plugin add <it> --enable`, the
#              user's command. The shell copies the plugin in, puts the icon
#              in the bar, and its service finds the binary, checks the sum,
#              links ~/.local/bin/om and starts the daemon, which writes a
#              fresh ~/.config/omaestro and offers the starter plugins.
#              The one difference from a user: no download.
#   uninstall  everything a user install leaves, and the older setups that
#              would answer instead (the systemd unit and ~/.cargo/bin/om of
#              `make install-systemd`, the `make plugin` link); the config and
#              state are moved to ~/.config/omaestro-backup-<time>, so the next
#              install starts like a new machine.

set -euo pipefail

cd "$(dirname "$0")/.."
root="$PWD"
id="io.github.iluxav.omaestro"
plugin_dir="$HOME/.config/omarchy/plugins/$id"
bin_dir="$HOME/.local/share/omaestro"
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/omaestro"
state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/omaestro"
stage="$root/target/plugin-install"

say() { printf '%s\n' "$*"; }

# A daemon that still answers, by whichever binary is around.
answering() {
  local om
  for om in "$HOME/.local/bin/om" "$HOME/.cargo/bin/om" "$(command -v om 2>/dev/null || true)"; do
    [[ -n "$om" && -x "$om" ]] && "$om" status >/dev/null 2>&1 && return 0
  done
  return 1
}

wait_quiet() {
  for _ in $(seq 50); do answering || return 0; sleep 0.1; done
  return 1
}

uninstall() {
  if [[ -L "$plugin_dir" ]]; then
    say "removing the plugin link of \`make plugin\`"
    omarchy plugin disable "$id" >/dev/null 2>&1 || true
    rm -f "$plugin_dir"
    omarchy-shell shell rescanPlugins >/dev/null 2>&1 || true
  elif [[ -d "$plugin_dir" ]]; then
    say "removing the Omarchy plugin"
    omarchy plugin remove "$id" --yes
  fi

  if [[ -f "$HOME/.config/systemd/user/omaestro.service" ]]; then
    say "removing the systemd user unit"
    systemctl --user disable --now omaestro >/dev/null 2>&1 || true
    rm -f "$HOME/.config/systemd/user/omaestro.service"
    systemctl --user daemon-reload
  fi

  # The plugin's service stops its daemon when the shell unloads it.
  if ! wait_quiet; then
    say "a daemon still answers; stopping it"
    for om in "$HOME/.local/bin/om" "$HOME/.cargo/bin/om"; do
      [[ -x "$om" ]] && "$om" stop >/dev/null 2>&1 || true
    done
    wait_quiet || say "warning: an omaestro daemon still answers (om status says who runs it)"
  fi

  if [[ -x "$HOME/.cargo/bin/om" ]]; then
    say "removing ~/.cargo/bin/om"
    cargo uninstall omaestro >/dev/null 2>&1 || rm -f "$HOME/.cargo/bin/om"
  fi
  # Only our link: a binary someone put in ~/.local/bin stays.
  if [[ -L "$HOME/.local/bin/om" && "$(readlink -f "$HOME/.local/bin/om")" == "$bin_dir/"* ]]; then
    rm -f "$HOME/.local/bin/om"
  fi
  rm -rf "$bin_dir" "$stage"

  if [[ -e "$config_dir" || -e "$state_dir" ]]; then
    backup="$config_dir-backup-$(date +%Y%m%d-%H%M%S)"
    mkdir -p "$backup"
    [[ -e "$config_dir" ]] && mv "$config_dir" "$backup/config"
    [[ -e "$state_dir" ]] && mv "$state_dir" "$backup/state"
    say "your rules and state are in $backup"
  fi

  if command -v om >/dev/null 2>&1; then
    say "note: another om is still on PATH: $(command -v om)"
  fi
  say "omaestro is uninstalled"
}

install() {
  if [[ -e "$plugin_dir" || -L "$plugin_dir" ]] || answering \
    || [[ -f "$HOME/.config/systemd/user/omaestro.service" || -x "$HOME/.cargo/bin/om" ]]; then
    say "omaestro is installed already; \`make uninstall\` first, for a clean start" >&2
    exit 1
  fi
  if [[ -e "$config_dir" ]]; then
    say "note: $config_dir exists, so this is not a first start (\`make uninstall\` moves it aside)"
  fi

  cargo build --release
  version="$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' manifest.json | head -1)"
  arch="$(uname -m)"
  bin="$bin_dir/om-$version-$arch"

  # Where the start script would have downloaded it to.
  mkdir -p "$bin_dir"
  command install -m755 target/release/om "$bin"
  sum="$(sha256sum "$bin" | awk '{print $1}')"

  # The plugin as the marketplace would fetch it: a git repository, here of
  # the working tree, with the local build pinned in place of the release's.
  rm -rf "$stage"
  mkdir -p "$stage"
  git ls-files -co --exclude-standard | tar -cf - -T - | tar -xf - -C "$stage"
  printf '%s %s\n' "$sum" "$arch" >"$stage/release.sha256"
  git -C "$stage" init -q
  git -C "$stage" add -A
  git -C "$stage" -c user.name=omaestro -c user.email=omaestro@localhost \
    commit -qm "omaestro $version, local build"

  # In a terminal it asks what it asks a user (trust, which bar section);
  # without one it would refuse, so it is told yes.
  local confirm=()
  [[ -t 0 ]] || confirm=(--yes)
  omarchy plugin add "file://$stage" --enable "${confirm[@]}"

  say "waiting for the plugin's service to start the daemon"
  for _ in $(seq 100); do
    if "$bin" status >/dev/null 2>&1; then
      "$bin" status | head -3
      say "installed: the icon is in the bar, SUPER+ALT+O opens the panel"
      return 0
    fi
    sleep 0.1
  done
  say "the daemon did not answer within 10 s; journalctl --user -t omarchy-shell | grep omaestro" >&2
  exit 1
}

case "${1:-}" in
  install) install ;;
  uninstall) uninstall ;;
  *) say "usage: $0 install|uninstall" >&2; exit 2 ;;
esac
