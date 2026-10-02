#!/usr/bin/env bash
# Live checks. Run inside the Hyprland session before claiming a milestone:
#
#   scripts/smoke.sh            everything a script can check
#   scripts/smoke.sh --press    also waits for you to press a hotkey by hand
#
# Nothing here changes your real Hyprland. Everything that binds keys,
# injects keystrokes or touches the clipboard runs inside a NESTED Hyprland:
# a second compositor in a window, with a scratch config, its own clipboard
# and its own binds. If it crashes, a window closes.
#
# What does reach the real session: a few notifications (they go over D-Bus),
# and a temporary systemd user unit that runs a daemon with no hotkeys.
# Neither ~/.config/omaestro nor a daemon that is already running is touched.
#
# M0: status, eval, hot reload, named triggers, rule errors, systemd unit.
# M1: hotkeys (bind, conflict, press, Hyprland reload, rule reload, exit),
#     selection, paste with clipboard restore, the model.
# Later: windows, events, apps, modes, timers, watchers, enable/disable, and
#     the plugin's panel loaded in quickshell (SMOKE_SHOTS=<dir> also saves
#     screenshots of it there).

set -u

cd "$(dirname "$0")/.."

PRESS=0
[[ "${1:-}" == "--press" ]] && PRESS=1

REAL_SIG="${HYPRLAND_INSTANCE_SIGNATURE:-}"
if [[ -z "$REAL_SIG" || -z "${WAYLAND_DISPLAY:-}" ]]; then
  echo "smoke: run this inside the Hyprland session (HYPRLAND_INSTANCE_SIGNATURE or WAYLAND_DISPLAY is unset)" >&2
  exit 2
fi
for tool in Hyprland hyprctl wtype wl-copy wl-paste dbus-monitor python3; do
  command -v "$tool" >/dev/null || { echo "smoke: $tool is needed" >&2; exit 2; }
done
TERMINAL=""
for candidate in foot kitty alacritty; do
  command -v "$candidate" >/dev/null && { TERMINAL="$candidate"; break; }
done
# How each one names its window class.
case "$TERMINAL" in
  foot) CLASS_FLAG="--app-id" ;;
  kitty) CLASS_FLAG="--app-id" ;;
  *) CLASS_FLAG="--class" ;;
esac
[[ -n "$TERMINAL" ]] || { echo "smoke: one of foot, kitty or alacritty is needed for the scratch window" >&2; exit 2; }

cargo build --quiet || exit 1
OM="$PWD/target/debug/om"

# Under XDG_RUNTIME_DIR: socket paths have a short length limit.
TMP="$(mktemp -d "$XDG_RUNTIME_DIR/omaestro-smoke.XXXXXX")"
CFG="$TMP/cfg"
LOG="$TMP/daemon.log"
BUS="$TMP/notifications.log"
UNIT="omaestro-smoke.service"
UNIT_DIR="$XDG_RUNTIME_DIR/systemd/user"
export OMAESTRO_SOCKET="$TMP/om.sock"
mkdir -p "$CFG/rules.d"

failed=0
daemon_pid=""
monitor_pid=""
nested_pid=""
scratch_pid=""
qs_pid=""
SIG=""
WL=""

# The variables a Hyprland start would export to systemd. A nested instance
# must leave them alone, or new apps in the real session would be pointed at
# the wrong compositor. Checked at the end, restored if they moved.
session_env() {
  systemctl --user show-environment | grep -E '^(WAYLAND_DISPLAY|HYPRLAND_INSTANCE_SIGNATURE|DISPLAY)=' | sort
}
SESSION_ENV_BEFORE="$(session_env)"

cleanup() {
  [[ -n "$daemon_pid" ]] && kill "$daemon_pid" 2>/dev/null
  [[ -n "$scratch_pid" ]] && kill "$scratch_pid" 2>/dev/null
  [[ -n "$qs_pid" ]] && kill "$qs_pid" 2>/dev/null
  [[ -n "$monitor_pid" ]] && kill "$monitor_pid" 2>/dev/null
  if [[ -n "$nested_pid" ]]; then
    kill "$nested_pid" 2>/dev/null
    for _ in $(seq 30); do kill -0 "$nested_pid" 2>/dev/null || break; sleep 0.1; done
    kill -9 "$nested_pid" 2>/dev/null
  fi
  if [[ -e "$UNIT_DIR/$UNIT" ]]; then
    systemctl --user stop "$UNIT" 2>/dev/null
    rm -f "$UNIT_DIR/$UNIT"
    systemctl --user daemon-reload
  fi
  if [[ "$(session_env)" != "$SESSION_ENV_BEFORE" ]]; then
    echo "smoke: the systemd session environment changed; restoring it" >&2
    while IFS= read -r line; do
      [[ -n "$line" ]] || continue
      systemctl --user set-environment "$line"
      dbus-update-activation-environment "$line" 2>/dev/null
    done <<<"$SESSION_ENV_BEFORE"
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT

pass() { echo "ok    $1"; }
fail() {
  echo "FAIL  $1"
  failed=1
}
skip() { echo "skip  $1"; }

# check <description> <command...>: passes when the command succeeds.
check() {
  local what="$1"
  shift
  if "$@" >/dev/null 2>&1; then pass "$what"; else fail "$what"; fi
}

# expect <description> <expected> <command...>: passes when the output matches exactly.
expect() {
  local what="$1" want="$2" got
  shift 2
  got="$("$@" 2>&1)"
  if [[ "$got" == "$want" ]]; then pass "$what"; else fail "$what (expected '$want', got '$got')"; fi
}

# wait_for <description> <command...>: retries for up to 5 seconds.
wait_for() {
  local what="$1"
  shift
  for _ in $(seq 50); do
    if "$@" >/dev/null 2>&1; then
      pass "$what"
      return 0
    fi
    sleep 0.1
  done
  fail "$what"
  return 1
}

notified() { grep -qF "$1" "$BUS"; }
status_has() { "$OM" status | grep -qF "$1"; }

# Runs a command against the nested compositor, never the real one.
in_nested() { WAYLAND_DISPLAY="$WL" HYPRLAND_INSTANCE_SIGNATURE="$SIG" "$@"; }
# Starts a scratch window in the nested compositor; `env`, not the function,
# so $scratch_pid is the terminal itself and killing it closes the window.
scratch() {
  env WAYLAND_DISPLAY="$WL" HYPRLAND_INSTANCE_SIGNATURE="$SIG" "$@" >/dev/null 2>&1 &
  scratch_pid=$!
}
# Descriptions of the nested instance's binds, one per line.
nested_binds() {
  in_nested hyprctl -j binds | python3 -c 'import json, sys
for bind in json.load(sys.stdin): print(bind.get("description", ""))'
}
nested_has_bind() { nested_binds | grep -qxF "$1"; }
nested_lacks_bind() { ! nested_binds | grep -qF "$1"; }
our_binds() { nested_binds | grep -c '^omaestro: '; }

start_daemon() {
  # `env`, not the in_nested function: $! has to be the daemon itself. Its
  # state (om.store, the disabled list) stays under $TMP, not in yours.
  env WAYLAND_DISPLAY="$WL" HYPRLAND_INSTANCE_SIGNATURE="$SIG" XDG_STATE_HOME="$TMP/state" \
    "$OM" daemon --foreground --config-dir "$CFG" >>"$LOG" 2>&1 &
  daemon_pid=$!
  wait_for "daemon answers status" "$OM" status
}

stop_daemon() {
  kill "$daemon_pid"
  wait "$daemon_pid" 2>/dev/null
  daemon_pid=""
}

# Every notification any program sends shows up here, whatever daemon draws them.
dbus-monitor --session "interface='org.freedesktop.Notifications',member='Notify'" >"$BUS" 2>&1 &
monitor_pid=$!

echo "# nested Hyprland"
cat >"$TMP/hyprland.lua" <<'EOF'
hl.monitor({ output = "", mode = "1280x720@60", position = "auto", scale = 1 })
hl.config({
  misc = { disable_hyprland_logo = true, disable_splash_rendering = true, disable_watchdog_warning = true },
  ecosystem = { no_update_news = true, no_donation_nag = true },
  xwayland = { enabled = false },
})
-- Stands in for a bind from the user's own config: omaestro has to refuse
-- this chord and must never remove it.
hl.bind("SUPER + Q", hl.dsp.exec_cmd("true"), { description = "smoke: the user's bind" })
EOF
# NO_SD_VARS: do not export this instance's display and signature to systemd
# and D-Bus. NO_SD_NOTIFY / NO_RT: it is not a service and needs no priority.
env -u HYPRLAND_INSTANCE_SIGNATURE HYPRLAND_NO_SD_VARS=1 HYPRLAND_NO_SD_NOTIFY=1 HYPRLAND_NO_RT=1 \
  Hyprland --config "$TMP/hyprland.lua" >"$TMP/hyprland.log" 2>&1 &
nested_pid=$!

find_nested() {
  local lock
  for lock in "$XDG_RUNTIME_DIR"/hypr/*/hyprland.lock; do
    [[ "$(sed -n 1p "$lock" 2>/dev/null)" == "$nested_pid" ]] || continue
    SIG="$(basename "$(dirname "$lock")")"
    WL="$(sed -n 2p "$lock")"
    [[ -n "$WL" && "$SIG" != "$REAL_SIG" ]] && return 0
  done
  return 1
}
for _ in $(seq 100); do find_nested && break; sleep 0.1; done
if [[ -z "$SIG" || -z "$WL" || "$SIG" == "$REAL_SIG" ]]; then
  echo "FAIL  the nested Hyprland did not start; its log:"
  tail -n 30 "$TMP/hyprland.log"
  exit 1
fi
wait_for "nested Hyprland answers ($WL)" in_nested hyprctl -j monitors || { tail -n 30 "$TMP/hyprland.log"; exit 1; }
expect "its config loaded without errors" "" in_nested hyprctl configerrors
check "the systemd session environment is untouched" test "$(session_env)" = "$SESSION_ENV_BEFORE"

echo "# daemon on $CFG"
start_daemon || { cat "$LOG"; exit 1; }
expect "eval 'return 1+1' prints 2" "2" "$OM" eval 'return 1+1'
expect "om repl answers a line" "om> 3
om> " sh -c "printf 'return 1+2\\n' | $OM repl"

echo "# hot reload"
cat >"$CFG/init.lua" <<'EOF'
om.trigger("smoke", function()
  om.notify("omaestro smoke", "named trigger fired")
end)
EOF
wait_for "init.lua is picked up after a save" status_has "files:     init.lua"
expect "the trigger is listed" "smoke  trigger  init.lua:1" "$OM" list
check "om trigger smoke" "$OM" trigger smoke
wait_for "its notification reached the desktop" notified "named trigger fired"

echo "# rule errors"
printf 'local x = 1\nerror("smoke test: a broken rule")\n' >"$CFG/rules.d/10-broken.lua"
wait_for "a broken rule file is notified with file and line" \
  notified "rules.d/10-broken.lua:2: smoke test: a broken rule"
check "the previous rules keep running" "$OM" trigger smoke
check "the daemon survived" kill -0 "$daemon_pid"

printf 'om.trigger("fails", function()\n  error("smoke test: a failing handler")\nend)\n' >"$CFG/rules.d/10-broken.lua"
wait_for "the fixed file loads" status_has "files:     init.lua, rules.d/10-broken.lua"
check "om trigger fails" "$OM" trigger fails
wait_for "a failing handler is notified with file and line" \
  notified "rules.d/10-broken.lua:2: smoke test: a failing handler"
check "the daemon survived" kill -0 "$daemon_pid"
rm "$CFG/rules.d/10-broken.lua"

echo "# hotkeys"
cat >"$CFG/init.lua" <<'EOF'
om.hotkey("SUPER + ALT + J", function()
  om.paste(om.selection():upper() .. "\n")
end)
om.hotkey("SUPER, K", function()
  om.notify("omaestro smoke", "hotkey K pressed")
end)
om.hotkey("SUPER + Q", function() end)
EOF
wait_for "a hotkey is bound in Hyprland" nested_has_bind "omaestro: init.lua:1"
wait_for "the older 'MODS, KEY' spelling binds too" nested_has_bind "omaestro: init.lua:4"
wait_for "a chord the user already bound is refused, with the reason" \
  notified "init.lua:7: SUPER + Q is already bound in Hyprland (smoke: the user's bind)"
expect "only the two free chords were bound" "2" our_binds
check "the user's bind is still there" nested_has_bind "smoke: the user's bind"

echo "# override: who wins a chord"
expect "om status says override is off" "override:  off (Hyprland's own binds win)" sh -c "'$OM' status | grep '^override'"
expect "om override alone says the same" "off (Hyprland's own binds win)" "$OM" override
check "om list names the refused chord" sh -c "'$OM' list | grep -q 'refused: SUPER + Q is already bound'"
expect "om override on takes the chord" "override on; 1 chord(s) taken from other binds" "$OM" override on
wait_for "our bind replaced the user's" nested_has_bind "omaestro: init.lua:7"
check "and the user's bind is gone for now" nested_lacks_bind "smoke: the user's bind"
wait_for "the user was told" notified "SUPER + Q now runs init.lua:7 instead of smoke: the user's bind"
check "om list shows what it overrides" sh -c "'$OM' list | grep -q 'overrides: smoke: the user'"
expect "and om override alone says on" "on (rules take chords Hyprland already has)" "$OM" override
in_nested hyprctl reload >/dev/null
sleep 0.5
wait_for "a Hyprland reload does not give it back while override is on" nested_has_bind "omaestro: init.lua:7"
check "the user's bind stays out" nested_lacks_bind "smoke: the user's bind"
expect "om override off" "override off; rules give way to Hyprland's own binds" "$OM" override off
wait_for "the user's bind is back (Hyprland reloaded its config)" nested_has_bind "smoke: the user's bind"
wait_for "and ours on that chord is gone" nested_lacks_bind "omaestro: init.lua:7"
two_binds() { [[ "$(our_binds)" == "2" ]]; }
wait_for "the other hotkeys came back with it" two_binds

echo "# selection and paste, in a scratch window"
# The window writes the first line it receives to a file.
scratch "$TERMINAL" sh -c "head -n 1 > '$TMP/pasted.txt'"
scratch_focused() { in_nested hyprctl -j activewindow | grep -qi "\"class\": \"$TERMINAL\""; }
wait_for "the scratch window has focus" scratch_focused
# wl-copy stays behind to serve the text; silence it for when the nested
# compositor goes away under it.
printf 'keep me' | in_nested wl-copy 2>/dev/null
printf 'smoke text' | in_nested wl-copy --primary 2>/dev/null
sleep 0.3
check "the hotkey's trigger fires through the socket" "$OM" trigger 'hotkey:SUPER+ALT+J'
pasted() { [[ "$(cat "$TMP/pasted.txt" 2>/dev/null)" == "SMOKE TEXT" ]]; }
wait_for "the selection came back upper-cased into the window" pasted
sleep 0.5
expect "the clipboard holds what it held before the paste" "keep me" in_nested wl-paste --no-newline

echo "# a real key press"
# Hyprland does not run binds for virtual keyboards, so no tool can press
# the chord for us. With --press, a human does it.
if [[ "$PRESS" == 1 ]]; then
  echo "      click into the nested Hyprland window and press SUPER+K (30 seconds)"
  pressed=0
  for _ in $(seq 300); do notified "hotkey K pressed" && { pressed=1; break; }; sleep 0.1; done
  if [[ "$pressed" == 1 ]]; then pass "pressing SUPER+K ran its handler"; else fail "pressing SUPER+K ran its handler"; fi
else
  skip "pressing the chord by hand (run with --press to include it)"
fi

echo "# Hyprland reloads its config"
in_nested hyprctl reload >/dev/null
two_binds() { [[ "$(our_binds)" == "2" ]]; }
wait_for "our binds are there after the reload" two_binds
check "the user's bind is still there" nested_has_bind "smoke: the user's bind"

echo "# rules reload"
cat >"$CFG/init.lua" <<'EOF'
om.hotkey("SUPER + ALT + J", function() end)
EOF
wait_for "a hotkey removed from the rules is unbound" nested_lacks_bind "omaestro: init.lua:4"
expect "the one still in the rules stays bound" "1" our_binds

echo "# enable and disable (what the panel does)"
list_json_ok() {
  "$OM" list --json | python3 -c 'import json, sys
rows = json.load(sys.stdin)
assert [r["id"] for r in rows] == ["hotkey:SUPER+ALT+J"], rows
assert rows[0]["enabled"] and rows[0]["detail"] == "SUPER + ALT + J", rows'
}
check "om list --json describes the hotkey" list_json_ok
expect "om disable answers" "disabled hotkey:SUPER+ALT+J" "$OM" disable 'hotkey:SUPER+ALT+J'
wait_for "a disabled hotkey is unbound" nested_lacks_bind "omaestro: init.lua:1"
check "om list marks it" sh -c "'$OM' list | grep -q '(disabled)'"
check "om status counts it" status_has "1 (1 disabled)"
check "firing it is refused with a hint" sh -c "'$OM' trigger 'hotkey:SUPER+ALT+J' 2>&1 | grep -q 'switched off'"
expect "the choice is on disk" '["hotkey:SUPER+ALT+J"]' python3 -c "import json; print(json.dumps(json.load(open('$TMP/state/omaestro/settings.json'))['disabled']))"
stop_daemon
start_daemon
check "and holds across a restart" sh -c "'$OM' list | grep -q '(disabled)'"
expect "om enable answers" "enabled hotkey:SUPER+ALT+J" "$OM" enable 'hotkey:SUPER+ALT+J'
wait_for "an enabled hotkey is bound again" nested_has_bind "omaestro: init.lua:1"
expect "an unknown id is refused" "om: no trigger named 'nope'" "$OM" disable nope
expect "scripts/om (the panel's launcher) reaches the daemon" "$("$OM" list)" env OMAESTRO_BIN="$OM" scripts/om list

echo "# the panel (Panel.qml in quickshell, in the nested Hyprland)"
# A global hotkey and an app-scoped one, so both kinds of row show.
cat >"$CFG/init.lua" <<'EOF'
om.hotkey("SUPER + ALT + J", function() end)
om.app_hotkey("^smoke%-panel$", "CTRL + S", function() end)
EOF
wait_for "the panel's rules loaded" status_has "triggers:  2"
# The Omarchy shell's own Commons and Ui modules, borrowed read-only; the
# panel is driven through quickshell's IPC the way a click would.
SHELL_SRC="${OMARCHY_PATH:-/usr/share/omarchy}/shell"
if command -v qs >/dev/null && [[ -d "$SHELL_SRC/Ui" && -d "$SHELL_SRC/Commons" ]]; then
  mkdir -p "$TMP/qs"
  ln -s "$SHELL_SRC/Commons" "$TMP/qs/Commons"
  ln -s "$SHELL_SRC/Ui" "$TMP/qs/Ui"
  cat >"$TMP/qs/shell.qml" <<EOF
import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
  Loader {
    id: panel
    source: "file://$PWD/Panel.qml"
    onLoaded: item.open("{}")
  }
  // The bar icon, alone in a strip at the top (no bar host: it is not
  // clickable here, its state and tooltip are what is checked).
  PanelWindow {
    anchors { top: true; left: true; right: true }
    implicitHeight: 32
    color: "#202020"
    Loader {
      id: widget
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      height: 32
      source: "file://$PWD/Widget.qml"
    }
  }
  IpcHandler {
    target: "smoke"
    function rules(): string {
      return panel.item.shownRules.map(function(r) { return r.id + "=" + r.enabled }).join(",")
    }
    function toggle(id: string): void {
      var rule = panel.item.rules.find(function(r) { return r.id === id })
      panel.item.setEnabled(id, !rule.enabled)
    }
    function reload(): void { panel.item.reload() }
    function message(): string { return panel.item.message }
    function error(): string { return panel.item.error }
    function setOverride(on: bool): void { panel.item.setOverride(on) }
    function override(): bool { return panel.item.overrideOn }
    function askOverride(): void { panel.item.askOverride() }
    function widget(): string { return widget.item ? widget.item.tooltip + "|" + widget.item.attention : "" }
    function cancelOverride(): void { panel.item.cancelOverride() }
    function labels(): string {
      return panel.item.grouped.map(function(r) { return r.section + ":" + panel.item.label(r) }).join(",")
    }
    function add(url: string): void { panel.item.addPlugin(url) }
    function remove(plugin: string): void { panel.item.removing = plugin; panel.item.remove() }
  }
}
EOF
  env WAYLAND_DISPLAY="$WL" HYPRLAND_INSTANCE_SIGNATURE="$SIG" OMAESTRO_BIN="$OM" \
    qs -p "$TMP/qs" >"$TMP/qs.log" 2>&1 &
  qs_pid=$!
  panel_ipc() { in_nested qs ipc -p "$TMP/qs" call smoke "$@" 2>/dev/null; }
  panel_rules() { [[ "$(panel_ipc rules)" == "$1" ]]; }
  shown=0
  for _ in $(seq 150); do panel_rules "app_hotkey:CTRL+S:class=^smoke%-panel$=true,hotkey:SUPER+ALT+J=true" && { shown=1; break; }; sleep 0.1; done
  if [[ "$shown" == 1 ]]; then
    pass "the panel loads with the shell's components and lists the hotkey"
    widget_says() { [[ "$(panel_ipc widget)" == "$1" ]]; }
    wait_for "the bar icon loads and counts the rules" widget_says "omaestro: 2 rules|false"
    if [[ -n "${SMOKE_SHOTS:-}" ]] && command -v grim >/dev/null; then
      mkdir -p "$SMOKE_SHOTS"
      in_nested grim "$SMOKE_SHOTS/panel-on.png" && echo "      screenshot: $SMOKE_SHOTS/panel-on.png"
    fi
    panel_ipc toggle 'hotkey:SUPER+ALT+J' >/dev/null
    wait_for "its switch runs om disable" nested_lacks_bind "omaestro: init.lua:1"
    wait_for "and the panel shows the rule as off" panel_rules "app_hotkey:CTRL+S:class=^smoke%-panel$=true,hotkey:SUPER+ALT+J=false"
    if [[ -n "${SMOKE_SHOTS:-}" ]] && command -v grim >/dev/null; then
      in_nested grim "$SMOKE_SHOTS/panel-off.png" && echo "      screenshot: $SMOKE_SHOTS/panel-off.png"
    fi
    panel_ipc toggle 'hotkey:SUPER+ALT+J' >/dev/null
    wait_for "switching it back on binds the hotkey again" nested_has_bind "omaestro: init.lua:1"
    wait_for "and the panel agrees" panel_rules "app_hotkey:CTRL+S:class=^smoke%-panel$=true,hotkey:SUPER+ALT+J=true"
    panel_ipc reload >/dev/null
    reload_shown() { [[ "$(panel_ipc message)" == "reloaded 1 file(s)" ]]; }
    wait_for "the reload button reloads and shows the daemon's answer" reload_shown
    expect "no error is shown" "" panel_ipc error
    if [[ -n "${SMOKE_SHOTS:-}" ]] && command -v grim >/dev/null; then
      # The warning that the override switch shows first.
      panel_ipc askOverride >/dev/null
      sleep 0.5
      in_nested grim "$SMOKE_SHOTS/panel-override-warning.png" && echo "      screenshot: $SMOKE_SHOTS/panel-override-warning.png"
      panel_ipc cancelOverride >/dev/null
    fi
    panel_ipc setOverride true >/dev/null
    override_is() { [[ "$(panel_ipc override)" == "$1" ]]; }
    wait_for "the override switch turns it on" override_is true
    wait_for "and the bar icon asks for attention" widget_says "omaestro: 2 rules
override is on: rules take chords Hyprland already has|true"
    if [[ -n "${SMOKE_SHOTS:-}" ]] && command -v grim >/dev/null; then
      sleep 0.5
      in_nested grim "$SMOKE_SHOTS/panel-override-on.png" && echo "      screenshot: $SMOKE_SHOTS/panel-override-on.png"
    fi
    expect "and the daemon agrees" "override:  on (rules take chords Hyprland already has)" sh -c "'$OM' status | grep '^override'"
    panel_ipc setOverride false >/dev/null
    wait_for "and off again" override_is false

    # A plugin's rules by their labels under its name; init.lua's panel
    # chord under omaestro's.
    mkdir -p "$CFG/lib"
    cp -r plugins/web-search "$CFG/lib/web-search"
    printf 'om.use("web-search").setup({})\n' >"$CFG/rules.d/web-search.lua"
    printf 'om.hotkey("SUPER + ALT + O", om.panel)\n' >"$CFG/init.lua"
    labels_are() { [[ "$(panel_ipc labels)" == "$1" ]]; }
    panel_ipc reload >/dev/null
    wait_for "rules show by their labels, under omaestro and the plugin's name" \
      labels_are "omaestro:omaestro menu,web-search  (plugin):Search the web"
    if [[ -n "${SMOKE_SHOTS:-}" ]] && command -v grim >/dev/null; then
      sleep 0.5
      in_nested grim "$SMOKE_SHOTS/panel-plugins.png" && echo "      screenshot: $SMOKE_SHOTS/panel-plugins.png"
    fi
    panel_ipc add "$PWD/plugins/text-tools" >/dev/null
    wait_for "the add field installs a plugin and lists its rules" \
      labels_are "omaestro:omaestro menu,text-tools  (plugin):Type today's date,text-tools  (plugin):Upper-case the selection,web-search  (plugin):Search the web"
    if [[ -n "${SMOKE_SHOTS:-}" ]] && command -v grim >/dev/null; then
      in_nested grim "$SMOKE_SHOTS/panel-added.png" && echo "      screenshot: $SMOKE_SHOTS/panel-added.png"
    fi
    panel_ipc add "$TMP/not-a-plugin" >/dev/null
    add_refused() { [[ "$(panel_ipc message)" == *"is not a directory"* ]]; }
    wait_for "a bad one says why" add_refused
    panel_ipc remove text-tools >/dev/null
    wait_for "remove uninstalls it" \
      labels_are "omaestro:omaestro menu,web-search  (plugin):Search the web"
    check "and its files are gone" test ! -e "$CFG/lib/text-tools"
    rm -rf "$CFG/lib" "$CFG/rules.d/web-search.lua"
  else
    fail "the panel did not load (quickshell log follows)"
    tail -n 30 "$TMP/qs.log"
  fi
  kill "$qs_pid" 2>/dev/null
  wait "$qs_pid" 2>/dev/null
  qs_pid=""
else
  skip "the panel (needs quickshell and the Omarchy shell sources)"
fi

echo "# focus, dispatch, type and key, in a scratch window"
cat >"$CFG/init.lua" <<'RULES'
om.on_focus({ class = "^smoke%-float$" }, function(win)
  om.dispatch("hl.dsp.window.float()")
  om.notify("omaestro smoke", "focused " .. win.class)
end)
om.on_blur({ class = "^smoke%-float$" }, function(win)
  om.notify("omaestro smoke", "blurred " .. win.class)
end)
om.trigger("type", function()
  om.type("typed 2026-09-30 ünïcode")
  om.key("Return")
end)
om.app_hotkey("^smoke%-float$", "SUPER + ALT + K", function() end)
RULES
wait_for "the focus rules loaded" status_has "triggers:  4"
check "an app hotkey is not bound while its window is absent" nested_lacks_bind "omaestro: init.lua:12"
scratch "$TERMINAL" "$CLASS_FLAG" smoke-float sh -c "head -n 1 > '$TMP/typed.txt'"
floating() { in_nested hyprctl -j activewindow | grep -q '"floating": true'; }
wait_for "focusing the window ran its on_focus handler" notified "focused smoke-float"
wait_for "which floated it with om.dispatch" floating
wait_for "the app hotkey is bound while its window has focus" nested_has_bind "omaestro: init.lua:12"
check "om trigger type" "$OM" trigger type
typed() { [[ "$(cat "$TMP/typed.txt" 2>/dev/null)" == "typed 2026-09-30 ünïcode" ]]; }
wait_for "om.type and om.key put a line into the window" typed
wait_for "the window closing ran its on_blur handler" notified "blurred smoke-float"
wait_for "and the app hotkey is unbound once it is gone" nested_lacks_bind "omaestro: init.lua:12"

echo "# window objects"
scratch "$TERMINAL" "$CLASS_FLAG" smoke-place sh -c "sleep 30"
placed_focused() { in_nested hyprctl -j activewindow | grep -q '"class": "smoke-place"'; }
wait_for "a scratch window has focus" placed_focused
check "om.window():place('left') is accepted" "$OM" eval 'om.window():place("left")'
left_half() {
  in_nested hyprctl -j activewindow | python3 -c 'import json, sys
w = json.load(sys.stdin)
m = [m for m in json.load(open(sys.argv[1])) if m["id"] == w["monitor"]][0]
usable_w = m["width"] // 2
sys.exit(0 if w["floating"] and w["at"][0] == m["x"] and abs(w["size"][0] - usable_w) <= 2 else 1)' "$TMP/monitors.json"
}
in_nested hyprctl -j monitors >"$TMP/monitors.json"
wait_for "the window floats on the left half of its monitor" left_half
expect "om.windows() finds it by class" "smoke-place" "$OM" eval 'return om.windows({class = "^smoke%-place$"})[1].class'
expect "om.monitor() names the nested output" "WAYLAND-1" "$OM" eval 'return om.monitor().name'
check "to_workspace moves it away" "$OM" eval 'om.window():to_workspace(5)'
on_workspace_5() { in_nested hyprctl -j clients | grep -q '"name": "5"'; }
wait_for "and the window is on workspace 5" on_workspace_5
kill "$scratch_pid" 2>/dev/null; scratch_pid=""

echo "# open, title, close, workspace events"
cat >"$CFG/init.lua" <<'RULES'
om.hotkey("SUPER + ALT + J", function() end)
om.on_open({ class = "^smoke%-events$" }, function(win) om.notify("omaestro smoke", "opened " .. win.class .. " on " .. win.workspace) end)
om.on_title({ class = "^smoke%-events$", title = "^Renamed$" }, function(win) om.notify("omaestro smoke", "titled " .. win.title) end)
om.on_close({ class = "^smoke%-events$" }, function(win) om.notify("omaestro smoke", "closed " .. win.class) end)
om.on_workspace(function(ws) om.notify("omaestro smoke", "workspace " .. ws.name) end)
RULES
wait_for "the event rules loaded" status_has "triggers:  5"
scratch "$TERMINAL" "$CLASS_FLAG" smoke-events sh -c "printf '\033]0;Renamed\007'; sleep 30"
wait_for "a window opening ran on_open with its workspace" notified "opened smoke-events on"
wait_for "its title change ran on_title" notified "titled Renamed"
in_nested hyprctl dispatch 'hl.dsp.focus({ workspace = "7" })' >/dev/null
wait_for "switching workspaces ran on_workspace" notified "workspace 7"
kill "$scratch_pid" 2>/dev/null; scratch_pid=""
wait_for "the window closing ran on_close with its class" notified "closed smoke-events"

echo "# apps"
expect "om.focus launches an app through Hyprland and waits for its window" "smoke-app" \
  "$OM" eval "local w = om.focus('^smoke%-app$', '$TERMINAL $CLASS_FLAG smoke-app sh -c \"sleep 30\"') return w and w.class"
app_focused() { in_nested hyprctl -j activewindow | grep -q '"class": "smoke-app"'; }
wait_for "and it has focus" app_focused
expect "om.apps() lists it" "1" "$OM" eval 'for _, a in ipairs(om.apps()) do if a.class == "smoke-app" then return a.count end end return 0'
check "the window closes through its object" "$OM" eval 'om.windows({class = "^smoke%-app$"})[1]:close()'
app_gone() { ! in_nested hyprctl -j clients | grep -q '"class": "smoke-app"'; }
wait_for "and is gone" app_gone

echo "# modes"
cat >"$CFG/init.lua" <<'RULES'
om.hotkey("SUPER + ALT + J", function() end)
om.mode("SUPER + ALT + W", { h = function() om.notify("omaestro smoke", "mode key h") end }, { hint = "smoke mode hint" })
RULES
wait_for "the mode's entry bind is there" nested_has_bind "omaestro: init.lua:2"
in_submap() { in_nested hyprctl -j binds | python3 -c 'import json, sys
sys.exit(0 if any(b.get("submap") == sys.argv[1] and b.get("key", "").upper() == sys.argv[2] for b in json.load(sys.stdin)) else 1)' "$1" "$2"; }
wait_for "its keys live in the submap" in_submap "om-super+alt+w" "H"
check "and Escape leaves it" in_submap "om-super+alt+w" "ESCAPE"
in_nested hyprctl dispatch 'hl.dsp.submap("om-super+alt+w")' >/dev/null
wait_for "entering the mode shows the hint" notified "smoke mode hint"
expect "Hyprland is in the mode" "om-super+alt+w" in_nested hyprctl repl 'return hl.get_current_submap()'
check "a mode key fires its trigger" "$OM" trigger 'mode:SUPER+ALT+W/H'
wait_for "and runs its handler" notified "mode key h"
in_nested hyprctl dispatch 'hl.dsp.submap("reset")' >/dev/null

echo "# timers, shell, clipboard, prompt"
cat >"$CFG/init.lua" <<'RULES'
om.hotkey("SUPER + ALT + J", function() end)
om.every("1s", function() om.notify("omaestro smoke", "tick") end)
RULES
printf 'prompt_command = "printf typed-%%s {label}"\n' >"$CFG/omaestro.toml"
wait_for "a timer ticks" notified "tick"
check "om.after schedules a one-shot" "$OM" eval 'om.after("1s", function() om.notify("omaestro smoke", "after fired") end)'
wait_for "which fires" notified "after fired"
printf 'prompt_command = "printf typed-%%s {label}"\nchoose_command = "printf %%s {options} | head -c 1"\n' >"$CFG/omaestro.toml"
sleep 0.6
expect "om.choose runs choose_command with the options as arguments" "b" "$OM" eval 'return om.choose("Pick", {"b", "c"})'
expect "om.store keeps a value across a reload" "7" "$OM" eval 'om.store.set("smoke", 7) return om.store.get("smoke")'
"$OM" reload >/dev/null
expect "and after it" "7" "$OM" eval 'return om.store.get("smoke")'
"$OM" eval 'om.store.set("smoke", nil)' >/dev/null
expect "om.shell returns the command's output" "shell-ok" "$OM" eval 'return om.shell("echo shell-ok")'
expect "om.set_clipboard and om.clipboard round-trip" "clip-ok" "$OM" eval 'om.set_clipboard("clip-ok") return om.clipboard()'
expect "om.prompt runs prompt_command with the label" "typed-hello" "$OM" eval 'return om.prompt("hello")'
printf '' >"$CFG/omaestro.toml"

echo "# leftovers after a crash"
kill -9 "$daemon_pid"
wait "$daemon_pid" 2>/dev/null
daemon_pid=""
expect "a killed daemon leaves its bind behind" "1" our_binds
doctor_says() { in_nested "$OM" doctor 2>&1 | grep -qF "$1"; }
check "om doctor reports the leftover" doctor_says "leftover bind SUPER + ALT + J"
in_nested "$OM" doctor --clear >/dev/null 2>&1
expect "om doctor --clear removes it" "0" our_binds
check "the user's bind is still there" nested_has_bind "smoke: the user's bind"
start_daemon
wait_for "the restarted daemon binds again" nested_has_bind "omaestro: init.lua:1"

echo "# http and spawn"
python3 -u -m http.server --bind 127.0.0.1 0 --directory "$TMP" >"$TMP/httpd.log" 2>&1 &
httpd_pid=$!
echo "served" >"$TMP/served.txt"
port=""
for _ in $(seq 50); do
  port=$(grep -oE "port [0-9]+" "$TMP/httpd.log" | head -1 | awk '{print $2}')
  [[ -n "$port" ]] && break
  sleep 0.1
done
if [[ -n "$port" ]]; then
  expect "om.http fetches a local file" "served" "$OM" eval "return om.http('http://127.0.0.1:$port/served.txt').body:trim()"
else
  fail "om.http (no local http server came up)"
fi
kill "$httpd_pid" 2>/dev/null
check "om.spawn starts a command in the background" "$OM" eval "om.spawn('touch $TMP/spawned')"
spawned() { test -e "$TMP/spawned"; }
wait_for "which ran" spawned

echo "# clipboard and file watchers, layout"
cat >"$CFG/init.lua" <<'RULES'
om.hotkey("SUPER + ALT + J", function() end)
om.on_clipboard(function(text) om.notify("omaestro smoke", "clipboard now " .. text) end)
RULES
printf 'om.on_file("%s", function(c) om.notify("omaestro smoke", "file " .. c.kind .. " " .. c.path) end)\n' "$TMP/watched" >"$CFG/rules.d/watch.lua"
mkdir -p "$TMP/watched"
wait_for "the watcher rules loaded" status_has "triggers:  3"
printf 'smoke clip' | in_nested wl-copy 2>/dev/null
wait_for "a clipboard change reaches on_clipboard with the text" notified "clipboard now smoke clip"
echo x >"$TMP/watched/new.txt"
wait_for "a new file reaches on_file" notified "file create $TMP/watched/new.txt"
rm "$CFG/rules.d/watch.lua"
scratch "$TERMINAL" "$CLASS_FLAG" smoke-layout sh -c "sleep 30"
layout_focused() { in_nested hyprctl -j activewindow | grep -q '"class": "smoke-layout"'; }
wait_for "a scratch window for the layout has focus" layout_focused
expect "om.layout places it" "1" "$OM" eval 'return om.layout({ {class = "^smoke%-layout$", place = "right"} })'
right_half() {
  in_nested hyprctl -j activewindow | python3 -c 'import json, sys
w = json.load(sys.stdin)
m = [m for m in json.load(open(sys.argv[1])) if m["id"] == w["monitor"]][0]
sys.exit(0 if w["floating"] and w["at"][0] >= m["x"] + m["width"] // 2 - 2 else 1)' "$TMP/monitors.json"
}
wait_for "on the right half" right_half
kill "$scratch_pid" 2>/dev/null; scratch_pid=""

echo "# typed triggers"
if id -nG | tr ' ' '\n' | grep -qx input; then
  cat >"$CFG/rules.d/typed.lua" <<'RULES'
om.on_typed(":smoke", function() om.notify("omaestro smoke", "typed trigger fired") end)
RULES
  wait_for "a typed rule loads without a permission error" status_has "rules.d/typed.lua"
  skip "typing the text by hand (no way to synthesize keys without uinput)"
  rm "$CFG/rules.d/typed.lua"
else
  skip "typed triggers: this user is not in the input group"
fi

echo "# plugins (om plugin: from repositories or directories into lib/)"
PLUG="$TMP/om-smoke-plugin"
mkdir -p "$PLUG"
(
  cd "$PLUG" && git init --quiet &&
    printf 'return { setup = function()\n  om.trigger("plugin-says", function() om.notify("omaestro smoke", "hello from the plugin") end)\nend }\n' >init.lua &&
    git -c user.name=smoke -c user.email=smoke@test add -A &&
    git -c user.name=smoke -c user.email=smoke@test commit --quiet -m first
) >/dev/null 2>&1
check "om plugin add takes a repository into lib/" env OMAESTRO_CONFIG_DIR="$CFG" "$OM" plugin add "$PLUG" --no-rule
check "om plugin list shows it with its Lua path" sh -c "OMAESTRO_CONFIG_DIR='$CFG' '$OM' plugin list | grep -q 'om-smoke-plugin .*lib/om-smoke-plugin/init.lua'"
cat >"$CFG/init.lua" <<'EOF'
om.use("om-smoke-plugin").setup()
EOF
wait_for "a rule loads it with om.use" status_has "triggers:  1"
check "om trigger plugin-says" "$OM" trigger plugin-says
wait_for "and the plugin's handler ran" notified "hello from the plugin"
# omaestro's own plugins come from a repository like anyone's; this
# checkout stands in for github.com/iluxav/omaestro (its committed HEAD).
export OMAESTRO_PLUGIN_REPO="$PWD"
check "om plugin available lists the repository's plugins" sh -c "OMAESTRO_CONFIG_DIR='$CFG' '$OM' plugin available | grep -q '^window-halves '"
check "om plugin add <name> fetches plugins/<name> from it and writes its rule" env OMAESTRO_CONFIG_DIR="$CFG" "$OM" plugin add window-halves
check "the rule file is there" test -f "$CFG/rules.d/window-halves.lua"
check "a copy of that one directory, with a record of where it came from" test -f "$CFG/lib/window-halves/.om-source.json" -a ! -e "$CFG/lib/window-halves/.git"
has_halves() { nested_binds | grep -qF "omaestro: lib/window-halves/init.lua"; }
lacks_halves() { ! has_halves; }
wait_for "its rule loads and its hotkeys bind, with lib/ origins" has_halves
check "om plugin list names its source" sh -c "OMAESTRO_CONFIG_DIR='$CFG' '$OM' plugin list | grep -q 'window-halves .* .*plugins/window-halves'"
check "om plugin remove drops it and its rule" env OMAESTRO_CONFIG_DIR="$CFG" "$OM" plugin remove window-halves
check "the rule file went with it" test ! -e "$CFG/rules.d/window-halves.lua"
wait_for "and its binds are gone" lacks_halves
check "a repository with --path, and a directory on disk, install the same way" \
  env OMAESTRO_CONFIG_DIR="$CFG" "$OM" plugin add "$PWD" --path plugins/reminders --no-rule
check "  (the directory)" env OMAESTRO_CONFIG_DIR="$CFG" "$OM" plugin add "$PWD/plugins/web-search" --no-rule
check "om plugin configure --set writes its options into its rule" \
  env OMAESTRO_CONFIG_DIR="$CFG" "$OM" plugin configure web-search --set chord="SUPER + CTRL + I"
check "  the rule has the chord" grep -q 'chord = "SUPER + CTRL + I",' "$CFG/rules.d/web-search.lua"
has_search() { nested_binds | grep -qF "omaestro: lib/web-search/init.lua"; }
wait_for "  and the daemon binds it" has_search
check "  both removed again" env OMAESTRO_CONFIG_DIR="$CFG" "$OM" plugin remove reminders
env OMAESTRO_CONFIG_DIR="$CFG" "$OM" plugin remove web-search >/dev/null 2>&1
unset OMAESTRO_PLUGIN_REPO
check "om plugin update takes the latest" env OMAESTRO_CONFIG_DIR="$CFG" "$OM" plugin update
check "om plugin new makes a plugin skeleton" env OMAESTRO_CONFIG_DIR="$CFG" "$OM" plugin new smoke-mine --no-edit
check "with an init.lua and a git repository" test -f "$CFG/lib/smoke-mine/init.lua" -a -d "$CFG/lib/smoke-mine/.git"
check "which loads as a plugin" "$OM" eval 'return om.use("smoke-mine") ~= nil'
check "om plugin remove refuses to drop uncommitted work" sh -c "! OMAESTRO_CONFIG_DIR='$CFG' '$OM' plugin remove smoke-mine 2>/dev/null"
check "and removes it with --force" env OMAESTRO_CONFIG_DIR="$CFG" "$OM" plugin remove smoke-mine --force
check "a clean clone goes without" env OMAESTRO_CONFIG_DIR="$CFG" "$OM" plugin remove om-smoke-plugin
wait_for "the rule that used it now fails to load, with the reason" notified "module 'om-smoke-plugin' not found"
# Leave a loadable file behind, or the next section's config change is
# refused along with the broken rule.
: >"$CFG/init.lua"
wait_for "an init.lua without it loads again" status_has "triggers:  0"

echo "# the model"
printf '[model]\nendpoint = "http://127.0.0.1:9/api/chat"\n' >"$CFG/omaestro.toml"
sleep 0.6
down="$("$OM" eval 'local text = om.llm("hi") return text' 2>&1)"
if [[ "$down" == *"model endpoint 127.0.0.1:9 is down"* ]]; then
  pass "an endpoint that is down says so"
else
  fail "an endpoint that is down says so (got '$down')"
fi
MODEL="${OMAESTRO_SMOKE_MODEL:-$(curl -s -m 2 http://127.0.0.1:11434/api/tags 2>/dev/null | python3 -c 'import json, sys
names = [m["name"] for m in json.load(sys.stdin)["models"]]
print(next((n for n in names if n.startswith("llama3.2")), names[0]))' 2>/dev/null)}"
if [[ -n "$MODEL" ]]; then
  printf '[model]\nname = "%s"\n' "$MODEL" >"$CFG/omaestro.toml"
  sleep 0.6
  answer="$("$OM" eval 'return om.llm("Reply with exactly one word: pong")' 2>&1)"
  if [[ $? -eq 0 && -n "$answer" ]]; then
    pass "om.llm got an answer from $MODEL: $(head -c 60 <<<"$answer" | tr '\n' ' ')"
  else
    fail "om.llm with $MODEL (got '$answer')"
  fi
else
  skip "om.llm against a real model: Ollama is not answering on 127.0.0.1:11434"
fi

echo "# shutdown"
check "om status names its supervisor: nothing, a script started it" status_has "under:     nothing"
check "om stop stops a daemon nobody supervises" "$OM" stop
wait "$daemon_pid" 2>/dev/null
daemon_pid=""
check "the socket is removed on exit" test ! -e "$OMAESTRO_SOCKET"
check "om status then says it is not running, with a failure exit" sh -c "! '$OM' status >/dev/null 2>&1 && '$OM' status | grep -q 'daemon:    not running'"
expect "our binds are removed on exit" "0" our_binds
check "the user's bind is still there" nested_has_bind "smoke: the user's bind"

echo "# systemd (real session, a daemon with no hotkeys)"
# The real unit file, pointed at this build and a config without hotkeys,
# placed in the runtime unit directory so nothing persists past this script.
mkdir -p "$UNIT_DIR" "$TMP/unit-cfg"
echo 'om.trigger("unit", function() end)' >"$TMP/unit-cfg/init.lua"
sed "s|^ExecStart=.*|ExecStart=$OM --socket $OMAESTRO_SOCKET daemon --config-dir $TMP/unit-cfg|" \
  systemd/omaestro.service >"$UNIT_DIR/$UNIT"
systemctl --user daemon-reload
export OMAESTRO_UNIT="$UNIT"
check "om status names the unit while nothing runs" sh -c "'$OM' status | grep -q 'unit:      $UNIT is inactive'"
check "om start starts the unit" "$OM" start
wait_for "the daemon answers under systemd (session environment present)" "$OM" status
check "it runs in the real session, not the nested one" status_has "hyprland:  $REAL_SIG"
check "om status names the unit as its supervisor" status_has "under:     the systemd user unit $UNIT"
check "the unit is active" systemctl --user is-active "$UNIT"
check "om start again just says so" sh -c "'$OM' start | grep -q 'already running'"
unit_pid() { "$OM" status --json 2>/dev/null | python3 -c 'import json, sys; print(json.load(sys.stdin)["pid"])' 2>/dev/null; }
pid_before="$(unit_pid)"
check "om restart restarts it" "$OM" restart
pid_changed() { [[ -n "$(unit_pid)" && "$(unit_pid)" != "$pid_before" ]]; }
wait_for "with a new pid" pid_changed
check "om stop stops it" "$OM" stop
check "the unit stops cleanly" test "$(systemctl --user show -p Result --value "$UNIT")" = success
check "and nothing answers" sh -c "! '$OM' status >/dev/null 2>&1"
unset OMAESTRO_UNIT

echo "# afterwards"
kill "$nested_pid" 2>/dev/null
for _ in $(seq 30); do kill -0 "$nested_pid" 2>/dev/null || break; sleep 0.1; done
check "the nested Hyprland exited" test ! -e "/proc/$nested_pid"
check "the systemd session environment is untouched" test "$(session_env)" = "$SESSION_ENV_BEFORE"

if [[ "$failed" -ne 0 ]]; then
  echo
  echo "smoke: FAILED. Daemon log:"
  cat "$LOG"
  exit 1
fi
echo
echo "smoke: all checks passed"
