<p align="center">
  <img src="assets/omaestro-logo-512.png" width="160" alt="omaestro">
</p>

<p align="center">
  <a href="https://github.com/iluxav/omaestro/actions/workflows/ci.yml"><img src="https://github.com/iluxav/omaestro/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/iluxav/omaestro/releases"><img src="https://img.shields.io/github/v/release/iluxav/omaestro?label=release" alt="Release"></a>
  <a href="https://omarchy.org"><img src="https://img.shields.io/badge/Omarchy-plugin-e07a5f" alt="Omarchy plugin"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT license"></a>
</p>

# omaestro

**Hammerspoon for Hyprland.** Small Lua rules that make your Omarchy desktop
do things for you: a hotkey that rewrites the text you selected with a local
model, a window that floats where you want it the moment it opens, a key
chord that means one thing in Firefox and nothing anywhere else, a reminder
every 45 minutes. One daemon, one `om` command, a panel in the Omarchy
shell, and plugins to start from.

```lua
-- Select text anywhere, press SUPER+ALT+J, get it back rewritten in place.
om.hotkey("SUPER + ALT + J", function()
  local text = om.selection()
  om.paste(om.llm("Rewrite this so it is clear and correct:\n\n" .. text))
end)
```

The project is `omaestro`; the command and the Lua table are both `om`.

## What it does

- **Hotkeys** in Hyprland's own syntax, registered live, never written to
  your Hyprland config. **App hotkeys** that exist only while one app has
  focus, so every other app keeps the chord. **Modes**: one chord, then
  single keys until Escape.
- **Windows**: react when a window opens, gets focus or changes title;
  float it, center it, send it to a workspace, place it on a half or a third
  of the screen, arrange a whole desk in one call.
- **Timers**: every interval, once after a delay, every day at a time.
- **The selection and the clipboard**: read them, replace them, watch them.
- **A local model**: `om.llm` through Ollama (or any OpenAI-compatible
  endpoint). Rewrite, summarize, translate what you selected.
- **The machine**: sleep and wake, USB devices, the battery, the network,
  files landing in a directory.
- **Typed text**: `:sig` typed anywhere becomes your signature.
- Shell commands, HTTP, notifications with buttons, a prompt and a chooser,
  persistent state. Errors show up as notifications with the file and line;
  the daemon never dies because of a rule.

## Install

### As an Omarchy plugin

```sh
omarchy plugin add https://github.com/iluxav/omaestro --enable
```

The plugin's service downloads the `om` binary for your CPU from this
repository's releases, checks it against the SHA256 pinned in the plugin,
links it as `~/.local/bin/om` so the command is in your terminal, and keeps
the daemon running while you are logged in. On its first start it offers
the starter plugins in a notification, one click. Or from a terminal:

```sh
om status                                       # the daemon, who runs it, what is loaded
om plugin add panel window-halves text-tools    # the starter set; SUPER+ALT+O opens the rules panel
```

### From source

```sh
git clone https://github.com/iluxav/omaestro && cd omaestro
make install           # cargo install → ~/.cargo/bin/om, plus a systemd user unit, started
om status
```

Either way, one daemon per session; the plugin does not start a second one
when the unit's is already answering.

Requirements: Omarchy (or any Arch setup with Hyprland 0.56 or newer, on
Wayland). `om doctor` checks the tools it uses (`hyprctl`, `wl-paste`,
`wl-copy`, `notify-send`, `wtype`). For `om.llm`: [Ollama](https://ollama.com)
with a model pulled, `ollama pull llama3.2`. For typed triggers: your user in
the `input` group.

## Start with the plugins

omaestro's own plugins live in this repository's [`plugins/`](plugins/)
directory and install exactly like anyone else's: `om` fetches the
directory from GitHub, checks it is a plugin, and copies it in. Each is a
small Lua module with options; install one and it works right away:

```sh
om plugin available                           # the list, fresh from GitHub, and which are installed
om plugin add window-halves                   # into ~/.config/omaestro/lib/, plus rules.d/window-halves.lua
om plugin add panel window-halves text-tools  # several at once: the starter set
```

| Plugin | What it does |
|---|---|
| `ai-text` | SUPER+ALT+J rewrites the selection with the local model; SUPER+ALT+M summarizes it; SUPER+ALT+T translates it |
| `window-halves` | CTRL+ALT+Left/Right/Up/Down put the window on a half, the whole screen or the center; SUPER+ALT+C floats and centers it, or tiles it back |
| `window-mode` | SUPER+ALT+W opens a window mode: h j k l halves, H L thirds, c center, m max, f float, Esc |
| `window-rules` | floats and centers Calculator when it opens; your own rules by class or title; a note when a monitor comes or goes |
| `apps` | SUPER+ALT+B brings Firefox to the front or starts it; SUPER+ALT+L arranges browser, editor and terminal; hotkeys scoped to one app |
| `text-tools` | SUPER+ALT+D types today's date; SUPER+ALT+U upper-cases the selection; your own snippets |
| `clipboard` | keeps the last ten clips, SUPER+ALT+V picks one to paste; SUPER+ALT+N appends the clipboard to `~/notes/clips.md` |
| `reminders` | SUPER+ALT+R asks for minutes and reminds you then; a stretch reminder every 45 minutes; a daily 17:30 note |
| `web-search` | SUPER+ALT+I asks for a query and opens it in the browser |
| `system-events` | notifications on wake, USB devices and a low battery; network changes in the journal |
| `downloads` | a notification when something lands in `~/Downloads` |
| `panel` | SUPER+ALT+O opens the rules panel of the Omarchy plugin |

`om plugin add` writes `~/.config/omaestro/rules.d/<name>.lua`, and that
file is where the options go; each plugin's README (in `lib/<name>/`, or
[`plugins/`](plugins/) here) lists them. Fixes and new plugins reach you
with `om plugin update`, no new `om` release needed:

```lua
-- rules.d/window-halves.lua
local window_halves = om.use("window-halves")
window_halves.setup({ chord = "SUPER + CTRL + " })
```

The chords stay clear of Omarchy's own. If one clashes with your config, the
rule is refused and says so, and every chord is an option.

## Write your own rules

Rules are Lua files in your config directory, loaded in this order:

```
~/.config/omaestro/init.lua        first
~/.config/omaestro/rules.d/*.lua   then, in name order
```

Create `~/.config/omaestro/rules.d/hello.lua`:

```lua
om.trigger("hello", function()
  om.notify("omaestro", "hello from a rule")
end)
```

Save it; the daemon reloads by itself. `om trigger hello` fires it. A few
more, each a complete file:

```lua
-- A hotkey. Chords are Hyprland's syntax.
om.hotkey("SUPER + ALT + U", function()
  om.paste(om.selection():upper())
end)

-- A chord for one app only: bound while Firefox has focus, unbound the
-- moment it loses it, so CTRL+S stays CTRL+S everywhere else.
om.app_hotkey("^firefox$", "CTRL + S", function()
  om.notify("firefox", "saved, the omaestro way")
end)

-- A window rule with logic: Spotify goes to workspace 9 when it opens.
om.on_open({ class = "^[Ss]potify$" }, function(win)
  win:to_workspace(9)
end)

-- Ask the model about the selection and show the answer.
om.hotkey("SUPER + ALT + M", function()
  local summary = om.llm(om.selection(), { system = "Summarize in three short sentences." })
  om.notify("Summary", summary)
end)
```

How rules behave:

- All files load into one Lua state; a global set in `init.lua` is visible
  in `rules.d/`.
- Saving any file reloads everything. If the new files fail to load, the
  previous rules keep running and you get a notification with the file and
  line. A rule that errors at runtime is notified the same way; the daemon
  keeps going.
- Handlers of one trigger run one at a time; different triggers do not wait
  for each other. A reload waits for running handlers.
- A rule can be switched off without editing it: `om disable <id>` (ids are
  in `om list`) or the panel. The choice survives reloads and restarts.

## Write a plugin, share a plugin

A plugin is a Lua module that rules configure instead of edit: a directory
with an `init.lua` that returns a table, usually with `setup(opts)`, and a
README. It lives in a public git repository, at its root or in a directory
of it (one repository can hold several, as this one does). The repository
is the package and a tag is a version; there is no registry and nothing to
build.

```sh
om plugin new my-plugin
```

That creates `~/.config/omaestro/lib/my-plugin/` with an `init.lua` (a
`setup(opts)` that binds one hotkey), a README, a git repository, and
`rules.d/my-plugin.lua` loading it, then opens `init.lua` in your editor.
Every save reloads it. The shape to keep:

```lua
-- init.lua
local M = {}

function M.setup(opts)
  opts = opts or {}
  om.hotkey(opts.chord or "SUPER + ALT + X", function()
    om.notify("my-plugin", "hello")
  end)
  return M
end

return M
```

Give every option a default, let `false` switch a chord off, and describe
the options in the README. When it is ready, push the repository to GitHub.
Others install it with:

```sh
om plugin add you/my-plugin                                  # a repository whose root is the plugin
om plugin add you/plugins/clock                              # the clock/ directory of you/plugins
om plugin add https://github.com/you/plugins/tree/main/clock # the same, as copied from the browser
om plugin add you/my-plugin --ref v1.0                       # a tag or branch
om plugin add https://git.example.com/x.git --path clock     # any git host
om plugin add ./my-plugin                                    # a directory on disk, as it is
om plugin update                                             # the latest of every plugin, or one by name
om plugin remove my-plugin                                   # also drops the rule it wrote
```

`om` keeps a record of where each plugin came from and what it installed:
`update` and `remove` refuse to throw away changes you made to a plugin's
files unless you add `--force`. A plugin that needs a newer `om` says so in
its `init.lua` with a line `-- requires om >= 0.2.0`, and `om plugin add`
refuses it on an older one.

A plugin is code that runs as you, with everything `om.*` can do. Install
plugins from people you trust, as you would a Hammerspoon Spoon.

## The panel

With the Omarchy plugin, `om panel` (or SUPER+ALT+O after `om plugin add
panel`) opens a panel in the shell: every rule with a switch, grouped by the
file or plugin it comes from; an app hotkey says which app and whether it is
bound right now; the override switch above them, red while it is on; and a
reload button.

## The `om` command

| Command | What |
|---|---|
| `om status [--json]` | the daemon: who runs it, what is loaded, the last error; when nothing answers, what would start it |
| `om start` / `om stop` / `om restart` | the daemon through its systemd unit (or whatever runs it); `restart` after a new build |
| `om list [--json]` | every rule: id, kind, origin, and `(disabled)`, `refused: …` or `overrides: …` |
| `om enable ID` / `om disable ID` | switch a rule on or off; kept across reloads and restarts |
| `om override [on\|off]` | let rules take chords Hyprland already has, or give them back; alone, which it is |
| `om trigger NAME` | fire a trigger by id (a hotkey's, or a named one) |
| `om reload` | reload the rule files now |
| `om eval 'lua'` / `om repl` | run Lua inside the daemon; inspect state |
| `om panel` | open or close the rules panel of the Omarchy plugin |
| `om plugin add NAME\|REPO\|URL\|DIR...` | install plugins: one of omaestro's by name, a GitHub repo or a directory in one, any git URL (`--path`, `--ref`), or a directory on disk |
| `om plugin available \| list \| new \| update \| remove` | omaestro's plugins, yours installed, start your own, take the latest, delete one |
| `om skill install \| show` | the omaestro skill for AI coding agents (Claude Code: `~/.claude/skills/omaestro`) |
| `om doctor [--clear]` | the session, the tools, leftover binds; `--clear` removes leftovers |
| `om daemon [--foreground]` | run the daemon in this terminal |

Every command takes `--socket PATH` (`OMAESTRO_SOCKET`) for a daemon on
another socket. Logs: `journalctl --user -u omaestro -f`; `OMAESTRO_LOG=debug`
for more.

## Settings

`~/.config/omaestro/omaestro.toml` is optional; these are the defaults:

```toml
[model]
endpoint = "http://127.0.0.1:11434/api/chat"  # Ollama; an OpenAI-compatible URL works too
name = "llama3.2"                              # ollama pull llama3.2
timeout_secs = 60
# api_key_env = "OPENAI_API_KEY"               # env var holding the key, for a remote endpoint

[paste]
chord = "ctrl+v"      # what pastes in most apps; terminals are known to take ctrl+shift+v
restore_ms = 300      # how long the pasted text stays in the clipboard before yours comes back

[paste.apps]          # apps that paste differently, by window class
# Emacs = "ctrl+y"

# prompt_command = "walker --dmenu -p {label}"               # what om.prompt runs
# choose_command = "omarchy-menu-select {label} {options}"   # what om.choose runs
```

Saving it reloads it; a mistake is reported like a rule error and the
previous settings stay. State (`om.store`, the disabled list, the override
switch) lives in `~/.local/state/omaestro/`.

## Good to know

**Your Hyprland config is never written.** Hotkeys are registered in the
running Hyprland and removed when the rule goes, on reload and on exit; they
show in `hyprctl binds` and Omarchy's keybindings menu as
`omaestro: rules.d/x.lua:3`.

**Who wins a chord.** Hyprland runs every bind on a chord, so omaestro
checks first. By default Hyprland's own binds win: a rule on a chord Omarchy
or your `bindings.lua` already uses is refused with a notification, stays
listed as `refused`, and binds the moment the chord is free (free it with
`hl.unbind("SUPER + J")` in your `bindings.lua`). `om override on` reverses
that: rules take their chords, a notification says what was replaced, and
the replaced bind comes back (Hyprland reloads its config) when the rule
goes, is disabled, override is turned off or the daemon exits.

**Pasting and typing.** `om.paste` goes through the clipboard and the app's
paste chord, and restores your clipboard afterwards; it is the sturdy route
for anything long or non-ASCII. `om.type` presses keys through a US keymap.
Both are pressed by Hyprland itself, so every app takes them. Right after a
hotkey, the first injection waits 400 ms for you to let go of the modifiers.

**The selection can be stale.** Text selected earlier in another window is
still the selection; a rule that replaces text acts on the window that has
focus now.

**Typed triggers** read the keyboards through evdev, read-only, only while a
rule has an `on_typed`, never logging more than the longest watched text.
They need your user in the `input` group (`sudo usermod -aG input $USER`,
then log in again).

**Privacy.** `om.llm` sends text to the endpoint in `omaestro.toml` and
nowhere else. The default is Ollama on `127.0.0.1`: nothing you select
leaves the machine unless you point it elsewhere. The control socket is
readable and writable only by you.

## Lua API reference

Everything lives under `om`. Triggers register a handler and return a handle
with `:remove()`; actions return their result and raise a Lua error on
failure.

```lua
-- Triggers
om.hotkey("SUPER + ALT + J", fn)                 -- a chord ("SUPER ALT, J" works too)
om.app_hotkey("^firefox$", "CTRL + S", fn)       -- only while a matching window has focus; or {class=, title=}
om.on_focus({class = "^firefox$"}, fn)           -- fn(win); matchers are Lua patterns, both must match
om.on_blur(matcher, fn) om.on_open(matcher, fn) om.on_close(matcher, fn) om.on_title(matcher, fn)
om.on_workspace(fn)                              -- fn({id, name})
om.on_monitor(fn)                                -- fn({name, change}); change: added, removed, focused
om.on_clipboard(fn)                              -- fn(text); "" for an image
om.on_sleep(fn) om.on_wake(fn)                   -- logind
om.on_usb(fn)                                    -- fn({action, device})
om.on_battery(fn)                                -- fn({percent, status}), polled every 30 s
om.on_network(fn)                                -- fn({line}), NetworkManager's words
om.on_file(path, fn)                             -- fn({path, kind}); recursive; kind: create, modify, remove
om.every("45m", fn)                              -- "30s", "5m", "1h", "1h30m"; a tick still running is skipped
om.after("10m", fn)                              -- once; the handle has :cancel()
om.at("17:30", fn)                               -- every day
om.mode("SUPER + ALT + W", {h = fn, ["SHIFT + h"] = fn}, {hint = "...", exit = {"q"}, once = false})
om.on_typed(":sig", fn)                          -- the text is erased, then fn runs
om.trigger("name", fn)                           -- `om trigger name`

-- Text, clipboard, keys
om.selection()                                   -- the primary selection, "" if none
om.clipboard() om.set_clipboard(text)
om.paste(text)                                   -- at the cursor, replacing a selection
om.type(text) om.key("ctrl+shift+t")

-- Model, shell, web
om.llm(prompt, {system = "...", model = "..."})  -- the configured model's answer
om.shell("cmd", {stdin = text, timeout = 5})     -- stdout; raises with stderr on failure
om.spawn("cmd")                                  -- in the background; returns the pid
om.http(url, {method = "POST", json = {...}, headers = {...}, timeout = 10})
                                                 -- {status, ok, body, headers, json}

-- Talking to you
om.notify(title, body)
om.notify(title, body, {actions = {yes = "Go", no = "Wait"}, timeout = 30})  -- returns the key pressed, or nil
om.prompt("label")                               -- a line from the menu, or nil
om.choose("label", {"a", "b"})                   -- one option, or nil
om.log(...)                                      -- the journal

-- State
om.store.get(key, default) om.store.set(key, value) om.store.all()   -- survives reloads and restarts
om.json.encode(value, {pretty = true}) om.json.decode(text)

-- Windows, apps, screens
om.window()                                      -- the focused window or nil: address, class, title, initial_class,
                                                 -- workspace, workspace_id, monitor, x, y, width, height, floating,
                                                 -- fullscreen_mode, pinned, pid, xwayland, focused
win:place("left")                                -- left, right, top, bottom, top-left, ..., left-third, middle-third,
                                                 -- right-third, left-two-thirds, right-two-thirds, center, max,
                                                 -- or {x = 0.25, y = 0, w = 0.5, h = 1} of the usable area
win:move(x, y) win:resize(w, h) win:center() win:float(true) win:pin() win:fullscreen("maximized")
win:focus() win:close() win:to_workspace(3) win:to_workspace("special:scratch", true) win:refresh()
om.windows({class = "^firefox$", workspace = 2})  -- every window, filtered by class, title, workspace, monitor
om.layout({{class = "^firefox$", place = "left"}, {class = "^code$", workspace = 2, place = "right", all = true}})
om.apps()                                        -- {class, count, windows}
om.launch("uwsm-app -- firefox")
om.focus("^firefox$", "uwsm-app -- firefox")     -- focus a matching window, or launch and wait up to 15 s
om.dispatch("hl.dsp.window.float()")             -- any Hyprland 0.56 dispatcher expression
om.monitor() om.monitor("DP-2") om.monitors()    -- {id, name, description, x, y, width, height, scale, transform, focused, workspace}
om.workspace() om.workspaces()                   -- {id, name, monitor, windows, has_fullscreen}
om.mouse() om.mouse_to(x, y)

-- Modules and strings
om.use("name", "you/repo")                       -- require from lib/, cloning the repository first if needed
("  x "):trim() ("a,b"):split(",") s:starts_with(p) s:ends_with(p)
```

## For developers

```sh
make check          # fmt, clippy, tests; no display or Hyprland needed
make smoke          # live checks in a nested Hyprland, run inside your session
make plugin         # link this checkout into the Omarchy shell as the plugin
make plugin-reload  # after editing Panel.qml or Service.qml (restarts the shell)
```

`scripts/smoke.sh` never touches your Hyprland: everything that binds keys,
injects keystrokes or uses the clipboard runs in a nested compositor with a
scratch config, and the panel is loaded in quickshell there. GitHub Actions
run the checks on every push.

Releasing is one command from a clean, pushed `main`:

```sh
make release                 # 0.1.0 -> 0.1.1; BUMP=minor or BUMP=major, or VERSION=1.2.3
make release-dry             # the checks and the plan, nothing changed
```

It sets the version in `Cargo.toml`, `Cargo.lock` and `manifest.json`,
commits, tags `vX.Y.Z`, pushes, then waits for the release workflow: `om`
built for x86_64 and aarch64, a GitHub release with both and their SHA256
sums, and the sums committed to `release.sha256` on `main` (pulled back for
you). The Omarchy plugin downloads the binary of the version in
`manifest.json` and refuses one whose sum does not match.

## Remove

```sh
omarchy plugin remove io.github.iluxav.omaestro   # the plugin, if installed that way
make uninstall                                    # the unit and ~/.cargo/bin/om, if built from source
rm -rf ~/.local/share/omaestro ~/.local/bin/om    # the downloaded binary and its link
rm -rf ~/.config/omaestro ~/.local/state/omaestro # your rules and state, only if you want them gone
```

## License

MIT
