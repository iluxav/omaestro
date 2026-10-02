#!/usr/bin/env bash
# `make preview`: preview.png for the marketplace, the panel with a few
# plugins loaded, taken in a nested Hyprland like scripts/smoke.sh does. The
# nested window is floated and resized in the real session so the whole card
# fits; nothing else of yours is touched.
set -u
cd "$(dirname "$0")/.."
OM="$PWD/target/debug/om"
OUT="${1:-preview.png}"
REAL_SIG="${HYPRLAND_INSTANCE_SIGNATURE:-}"
TMP="$(mktemp -d "$XDG_RUNTIME_DIR/omaestro-preview.XXXXXX")"
CFG="$TMP/cfg"; mkdir -p "$CFG/rules.d"
export OMAESTRO_SOCKET="$TMP/om.sock"
nested_pid=""; daemon_pid=""; qs_pid=""
cleanup() {
  [[ -n "$qs_pid" ]] && kill "$qs_pid" 2>/dev/null
  [[ -n "$daemon_pid" ]] && kill "$daemon_pid" 2>/dev/null
  if [[ -n "$nested_pid" ]]; then kill "$nested_pid" 2>/dev/null; for _ in $(seq 30); do kill -0 "$nested_pid" 2>/dev/null || break; sleep 0.1; done; kill -9 "$nested_pid" 2>/dev/null; fi
  rm -rf "$TMP"
}
trap cleanup EXIT
cat >"$TMP/hyprland.lua" <<'EOF'
hl.monitor({ output = "", mode = "1400x1150@60", position = "auto", scale = 1 })
hl.config({
  misc = { disable_hyprland_logo = true, disable_splash_rendering = true, disable_watchdog_warning = true },
  cursor = { invisible = true },
  ecosystem = { no_update_news = true, no_donation_nag = true },
  xwayland = { enabled = false },
})
EOF
env -u HYPRLAND_INSTANCE_SIGNATURE HYPRLAND_NO_SD_VARS=1 HYPRLAND_NO_SD_NOTIFY=1 HYPRLAND_NO_RT=1 \
  Hyprland --config "$TMP/hyprland.lua" >"$TMP/hyprland.log" 2>&1 &
nested_pid=$!
SIG=""; WL=""
for _ in $(seq 100); do
  for lock in "$XDG_RUNTIME_DIR"/hypr/*/hyprland.lock; do
    [[ "$(sed -n 1p "$lock" 2>/dev/null)" == "$nested_pid" ]] || continue
    SIG="$(basename "$(dirname "$lock")")"; WL="$(sed -n 2p "$lock")"
  done
  [[ -n "$SIG" && -n "$WL" && "$SIG" != "$REAL_SIG" ]] && break
  sleep 0.1
done
[[ -n "$SIG" && "$SIG" != "$REAL_SIG" ]] || { echo "no nested instance"; exit 1; }
in_nested() { WAYLAND_DISPLAY="$WL" HYPRLAND_INSTANCE_SIGNATURE="$SIG" "$@"; }
for _ in $(seq 50); do in_nested hyprctl -j monitors >/dev/null 2>&1 && break; sleep 0.1; done
# The pointer in a corner, off the card, so no row is shown hovered.
in_nested hyprctl dispatch 'hl.dsp.cursor.move({ x = 1, y = 1 })' >/dev/null

# The nested compositor is a window of the real session, sized by its
# layout; float it and make it tall enough for the whole card (the same
# dispatchers om.window():float()/resize()/center() use).
addr=""
for _ in $(seq 50); do
  addr="$(hyprctl -j clients | jq -r --argjson pid "$nested_pid" '[.[] | select(.pid == $pid)] | .[0].address // empty')"
  [[ -n "$addr" ]] && break
  sleep 0.1
done
if [[ -n "$addr" ]]; then
  hyprctl dispatch "hl.dsp.window.float({ action = \"enable\", window = \"address:$addr\" })" >/dev/null
  hyprctl dispatch "hl.dsp.window.resize({ x = 1400, y = 1150, window = \"address:$addr\" })" >/dev/null
  hyprctl dispatch "hl.dsp.window.center({ window = \"address:$addr\" })" >/dev/null
  sleep 1.5
fi

# Rules as a user has them: init.lua's panel chord, a few plugins (from this
# checkout, so what is shown is what will ship), and one hand-written app
# hotkey.
printf 'om.hotkey("SUPER + ALT + O", om.panel)\n' >"$CFG/init.lua"
for p in window-halves ai-text text-tools; do OMAESTRO_CONFIG_DIR="$CFG" "$OM" plugin add "$PWD/plugins/$p" >/dev/null; done
cat >"$CFG/rules.d/firefox.lua" <<'EOF'
om.app_hotkey("^firefox$", "CTRL + S", function() om.notify("firefox", "saved") end, { label = "Save in Firefox" })
EOF
env WAYLAND_DISPLAY="$WL" HYPRLAND_INSTANCE_SIGNATURE="$SIG" XDG_STATE_HOME="$TMP/state" \
  "$OM" daemon --foreground --config-dir "$CFG" >"$TMP/daemon.log" 2>&1 &
daemon_pid=$!
for _ in $(seq 50); do "$OM" status >/dev/null 2>&1 && break; sleep 0.1; done
for _ in $(seq 50); do [[ "$("$OM" list 2>/dev/null | wc -l)" -ge 12 ]] && break; sleep 0.1; done

SHELL_SRC="${OMARCHY_PATH:-/usr/share/omarchy}/shell"
mkdir -p "$TMP/qs"; ln -s "$SHELL_SRC/Commons" "$TMP/qs/Commons"; ln -s "$SHELL_SRC/Ui" "$TMP/qs/Ui"
cat >"$TMP/qs/shell.qml" <<EOF
import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
  Loader { id: panel; source: "file://$PWD/Panel.qml"; onLoaded: item.open("{}") }
  IpcHandler {
    target: "preview"
    function count(): int { return panel.item.shownRules.length }
  }
}
EOF
env WAYLAND_DISPLAY="$WL" HYPRLAND_INSTANCE_SIGNATURE="$SIG" OMAESTRO_BIN="$OM" qs -p "$TMP/qs" >"$TMP/qs.log" 2>&1 &
qs_pid=$!
for _ in $(seq 150); do
  n="$(in_nested qs ipc -p "$TMP/qs" call preview count 2>/dev/null)"
  [[ "${n:-0}" -ge 12 ]] && break
  sleep 0.1
done
echo "panel shows ${n:-0} rules; the pointer is at $(in_nested hyprctl cursorpos 2>/dev/null)"
sleep 1
in_nested grim "$TMP/full.png"; cp "$TMP/full.png" "${OUT%.png}-full.png"
# Keep the card: trim the uniform scrim, then a little of it back as a frame.
magick "$TMP/full.png" -fuzz 3% -trim +repage -bordercolor '#101010' -border 28 "$OUT"
echo "wrote $OUT"; tail -3 "$TMP/qs.log" | grep -i "error\|warn" || true
