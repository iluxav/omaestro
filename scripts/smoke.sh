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
# Descriptions of the nested instance's binds, one per line.
nested_binds() {
  in_nested hyprctl -j binds | python3 -c 'import json, sys
for bind in json.load(sys.stdin): print(bind.get("description", ""))'
}
nested_has_bind() { nested_binds | grep -qxF "$1"; }
nested_lacks_bind() { ! nested_binds | grep -qF "$1"; }
our_binds() { nested_binds | grep -c '^omaestro: '; }

start_daemon() {
  # `env`, not the in_nested function: $! has to be the daemon itself.
  env WAYLAND_DISPLAY="$WL" HYPRLAND_INSTANCE_SIGNATURE="$SIG" \
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

echo "# selection and paste, in a scratch window"
# The window writes the first line it receives to a file.
in_nested "$TERMINAL" sh -c "head -n 1 > '$TMP/pasted.txt'" >/dev/null 2>&1 &
scratch_pid=$!
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
RULES
wait_for "the focus rules loaded" status_has "triggers:  3"
in_nested "$TERMINAL" "$CLASS_FLAG" smoke-float sh -c "head -n 1 > '$TMP/typed.txt'" >/dev/null 2>&1 &
scratch_pid=$!
floating() { in_nested hyprctl -j activewindow | grep -q '"floating": true'; }
wait_for "focusing the window ran its on_focus handler" notified "focused smoke-float"
wait_for "which floated it with om.dispatch" floating
check "om trigger type" "$OM" trigger type
typed() { [[ "$(cat "$TMP/typed.txt" 2>/dev/null)" == "typed 2026-09-30 ünïcode" ]]; }
wait_for "om.type and om.key put a line into the window" typed
wait_for "the window closing ran its on_blur handler" notified "blurred smoke-float"

echo "# timers, shell, clipboard, prompt"
cat >"$CFG/init.lua" <<'RULES'
om.hotkey("SUPER + ALT + J", function() end)
om.every("1s", function() om.notify("omaestro smoke", "tick") end)
RULES
printf 'prompt_command = "printf typed-%%s {label}"\n' >"$CFG/omaestro.toml"
wait_for "a timer ticks" notified "tick"
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
stop_daemon
check "the socket is removed on exit" test ! -e "$OMAESTRO_SOCKET"
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
check "the unit starts" systemctl --user start "$UNIT"
wait_for "the daemon answers under systemd (session environment present)" "$OM" status
check "it runs in the real session, not the nested one" status_has "hyprland:  $REAL_SIG"
check "the unit is active" systemctl --user is-active "$UNIT"
systemctl --user stop "$UNIT"
check "the unit stops cleanly" test "$(systemctl --user show -p Result --value "$UNIT")" = success

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
