# omaestro

Hammerspoon for Hyprland. A daemon that loads Lua rule files and runs them
against your desktop: when this hotkey, focus change, timer or selection
happens, do these steps.

The project is `omaestro`; the command and the Lua table are both `om`.

```lua
-- Select text anywhere, press the chord, get it back rewritten in place.
om.hotkey("SUPER + ALT + J", function()
  local text = om.selection()
  om.paste(om.llm("Rewrite this so it is clear and correct:\n\n" .. text))
end)
```

## Status

Working today: hotkeys, modes, window objects and layouts, window, focus,
workspace and monitor events, timers, typed-text triggers, the selection
and clipboard (with a watcher), file and system watchers (sleep, USB,
battery, network), the local model, paste, typing, key presses, shell
commands, HTTP, a prompt, a chooser, notifications with buttons, persistent
state, Hyprland dispatchers, and a panel in the Omarchy shell that switches
rules on and off. Not yet: idle detection. The release binary and the
marketplace listing are in preparation; building from source works now.

## Requirements

- Omarchy (or any Arch setup) with Hyprland 0.56 or newer, on Wayland.
- `hyprctl`, `wtype`, `wl-copy`, `wl-paste` and `notify-send` on PATH (all
  present on Omarchy). `om doctor` checks them.
- For `om.on_typed`: your user in the `input` group (see Typed triggers).
- For `om.llm`: [Ollama](https://ollama.com) running locally with a model
  pulled, or any OpenAI-compatible endpoint.
- A Rust toolchain to build.

## Install

As an Omarchy plugin (the service keeps the daemon running while the shell
is up; the binary is a prebuilt release, verified against a pinned SHA256):

```sh
omarchy plugin add https://github.com/iluxav/omaestro --enable
```

Or from source, with a systemd user unit:

```sh
cargo install --path .
install -Dm644 systemd/omaestro.service ~/.config/systemd/user/omaestro.service
systemctl --user enable --now omaestro
```

Pick one: the plugin does not start a second daemon if the unit's is
already answering.

The plugin also adds a panel to the shell: every rule with a switch,
grouped by file or plugin (an app hotkey says which app and whether it is
bound right now), the override switch above them, red while it is on, and a
reload button. `om panel` opens and closes it, as does
`omarchy-shell shell toggle io.github.iluxav.omaestro`; put either on a key
with the `panel` plugin (`om plugin add panel`, SUPER+ALT+O) or a bind in
`~/.config/hypr/bindings.lua`. It talks to whichever daemon is
running, the plugin's or the unit's.

`cargo install` puts `om` in `~/.cargo/bin`, which is where the unit expects
it. Check that everything is in place:

```sh
om doctor
om status      # who runs it and what is loaded
om restart     # after a new build; om start and om stop drive the unit
```

`om restart` works under the Omarchy plugin too (the shell's service starts
the daemon again). To run it in a terminal instead: `om daemon --foreground`.

The daemon must be started inside the Hyprland session. If
`HYPRLAND_INSTANCE_SIGNATURE` is unset it refuses to start and says so.

## Remove

```sh
omarchy plugin remove io.github.iluxav.omaestro     # the plugin, if installed that way
systemctl --user disable --now omaestro             # the unit, if installed that way
rm ~/.config/systemd/user/omaestro.service
cargo uninstall omaestro
rm -rf ~/.local/share/omaestro                      # the downloaded release binary
```

Your rules in `~/.config/omaestro/` are yours; delete them if you want them gone.

## First rule

Rules live in your config directory:

```
~/.config/omaestro/init.lua        loaded first
~/.config/omaestro/rules.d/*.lua   loaded after it, in name order
```

Create `~/.config/omaestro/rules.d/hello.lua`:

```lua
om.trigger("hello", function()
  om.notify("omaestro", "hello from a rule")
end)
```

Save it. The daemon notices the change and reloads; there is nothing to
restart. Then:

```sh
om trigger hello
```

Twelve plugins ship inside `om`: each is a directory under `plugins/` in
this repository (`init.lua` plus a README) that you can read as an example,
install as it is, or change in your copy under `~/.config/omaestro/lib/`.

```sh
om plugin available              # the list, with what is installed
om plugin add window-halves      # copies it into lib/ and writes rules.d/window-halves.lua
```

The rule file is where the options go; each plugin's README lists them.

| Plugin | What it does |
|---|---|
| `ai-text` | SUPER+ALT+J rewrites the selection with the local model (the rule from the top of this page); SUPER+ALT+M summarizes it; SUPER+ALT+T translates it |
| `window-halves` | CTRL+ALT+Left/Right/Up/Down put the window on a half, the whole screen or the center; SUPER+ALT+C floats and centers it, or tiles it back |
| `window-mode` | SUPER+ALT+W opens a window mode: h j k l halves, H L thirds, c center, m max, f float, Esc |
| `window-rules` | floats and centers Calculator when it opens; your own rules by class or title; a note when a monitor comes or goes |
| `apps` | SUPER+ALT+B brings Firefox to the front or starts it; SUPER+ALT+L arranges browser, editor and terminal |
| `text-tools` | SUPER+ALT+D types today's date; SUPER+ALT+U upper-cases the selection; your own snippets |
| `clipboard` | keeps the last ten clips, SUPER+ALT+V picks one to paste; SUPER+ALT+N appends the clipboard to `~/notes/clips.md` |
| `reminders` | SUPER+ALT+R asks for minutes and reminds you then; a stretch reminder every 45 minutes; a daily 17:30 note |
| `web-search` | SUPER+ALT+I asks for a query and opens it in the browser |
| `system-events` | notifications on wake, USB devices and a low battery; network changes in the journal |
| `downloads` | a notification when something lands in `~/Downloads` |
| `panel` | SUPER+ALT+O opens the rules panel of the Omarchy plugin |

The chords stay clear of Omarchy's own (it uses every SUPER+arrow
combination and SUPER+ALT+S, among others). If one clashes with your
config, the rule is refused and says so; every chord is an option.

## How rules behave

- All files load into one Lua state, so a global set in `init.lua` is visible
  in `rules.d/`.
- Saving any rule file reloads everything into a fresh state. If the new
  files fail to load, the previous rules keep running and you get a
  notification.
- An error in a rule never stops the daemon. It is logged and shown as a
  notification with the file and line: `rules.d/hello.lua:2: <message>`.
- Handlers of one trigger run one at a time. Different triggers do not wait
  for each other.
- A reload waits for running handlers to finish.

## Switching rules off

A rule can be switched off without editing it, from the panel or the
command line:

```sh
om list                          # every rule, with its id
om disable 'hotkey:SUPER+ALT+J'  # the hotkey is unbound, a timer stops, an event rule is skipped
om enable 'hotkey:SUPER+ALT+J'
```

The choice is kept in `~/.local/state/omaestro/settings.json` and holds
across reloads and restarts, until you enable the rule again. Ids come from
the rule itself, not its place in the file: `hotkey:SUPER+ALT+J`,
`on_focus:class=firefox`, `every:5m`, `at:17:30`, `on_clipboard`, a named
trigger's name. Two rules that look alike get `#2`, `#3` after the second.
Switching a mode off takes its keys with it. `om trigger` on a rule that is
off says so instead of firing it.

## Hotkeys and your Hyprland config

omaestro never writes to `~/.config/hypr`. `om.hotkey` registers the bind in
the running Hyprland, and it is removed again when the rule goes away, on
reload and when the daemon stops. If Hyprland reloads its own config, the
binds are put back.

### Who wins a chord

Hyprland runs every bind on a chord, so two binds on one chord both fire.
omaestro therefore checks `hyprctl -j binds` before binding, and by default
**Hyprland's own binds win**: a rule whose chord Omarchy or your
`bindings.lua` already uses is refused. The rule stays listed (`om list`
shows `refused: ...`, the panel shows it in red) with a notification naming
the existing bind, and it is bound the moment the chord becomes free. To
give such a chord to a rule, free it in your Hyprland config. On Omarchy,
`SUPER + J` is "Toggle window split"; to hand it over, add this to
`~/.config/hypr/bindings.lua`:

```lua
hl.unbind("SUPER + J")
```

Or let the rules win everywhere:

```sh
om override on       # or the switch at the bottom of the panel; `om override` alone says which it is
```

From then on a rule takes its chord: Hyprland's bind on it is removed, a
notification says what was replaced, and `om list` shows `overrides: <what>`.
The replaced shortcut stops working for as long as the rule is loaded and
switched on. When the rule goes away, is disabled, override is turned off or
the daemon exits, Hyprland reloads its config (`hyprctl reload`) to bring
the bind back: with the Lua config there is no other way to recreate a bind
that isn't ours. If the daemon crashes instead, the bind stays gone until
the next Hyprland reload or daemon start. The setting is kept in
`~/.local/state/omaestro/settings.json`.

Your hotkeys show up in Hyprland's bind list (and Omarchy's keybindings menu)
with a description like `omaestro: rules.d/rewrite.lua:3`. An `om.app_hotkey`
appears there only while its app has focus: the bind is made on focus and
removed on blur, which is how other apps keep the chord. A chord is either
global or per app, never both.

Run one daemon per Hyprland session.

## Typed triggers

`om.on_typed` reads the keyboards through evdev (`/dev/input/event*`),
read-only, nothing grabbed: Hyprland and every app still get every key.
That needs your user in the `input` group:

```sh
sudo usermod -aG input $USER    # then log out and in again
```

Keys are only read while at least one rule has an `on_typed`; the daemon
keeps no more than the longest watched text (32 characters at most), never
logs it, and drops it on Enter, arrows and other non-text keys. The key
mapping assumes a US layout. The watched text is erased with Backspace
presses before the handler runs, so `om.type`/`om.paste` in the handler
replace it. Built with the `typed` feature, which is on by default;
`cargo build --no-default-features` leaves it out, and `om.on_typed` then
raises an error.

## Privacy

`om.llm` sends the prompt to the endpoint in `omaestro.toml` and nowhere
else. The default is Ollama on `127.0.0.1`, so by default nothing you select
leaves the machine. If you point `endpoint` at a remote service, the text
you pass to `om.llm` goes to that service.

## Settings

`~/.config/omaestro/omaestro.toml` is optional. Everything in it has a
default, shown here:

```toml
[model]
endpoint = "http://127.0.0.1:11434/api/chat"  # Ollama; an OpenAI-compatible URL works too
name = "llama3.2"                              # must be pulled: ollama pull llama3.2
timeout_secs = 60
# api_key_env = "OPENAI_API_KEY"               # name of the env var holding the key, if needed

[paste]
chord = "ctrl+v"      # what pastes in most apps
restore_ms = 300      # how long the pasted text stays in the clipboard

[paste.apps]          # apps that paste differently, by window class
# Emacs = "ctrl+y"

# prompt_command = "walker --dmenu -p {label}"  # what om.prompt runs; {label} is the prompt text
# choose_command = "omarchy-menu-select {label} {options}"  # what om.choose runs; without {options}
                                                            # the choices go to stdin, one per line
```

`om.store` keeps its file at `~/.local/state/omaestro/store.json`
(`$XDG_STATE_HOME/omaestro`).

Terminals (Alacritty, kitty, foot, Ghostty, WezTerm) are already known to
paste with `ctrl+shift+v`. Saving the file reloads it; a mistake in it is
reported like a rule error and the previous settings stay.

## Plugins

A plugin is a Lua module that rules configure instead of edit: a directory
under `~/.config/omaestro/lib/<name>/` with an `init.lua` that returns a
table, usually with a `setup(opts)`. Twelve ship inside `om` (the table
above); any git repository of that shape works too, and then the repository
is the package and a tag the version. No archives.

```sh
om plugin available                               # the plugins that ship with om, and which are installed
om plugin add window-halves                       # one of them: copied into lib/, rules.d/window-halves.lua written
om plugin add https://github.com/you/om-thing     # a repository; or: you/om-thing
om plugin add you/om-thing --ref v1.0             # pin a tag or branch
om plugin list                                    # name, version, Lua path, source
om plugin update                                  # git pull them all, or one by name
om plugin remove om-thing                         # drops the rule it wrote too; refuses to lose uncommitted work
```

`add` writes `rules.d/<name>.lua`, so the plugin runs with its defaults at
once, and that file is where the options go:

```lua
-- rules.d/window-halves.lua
local window_halves = om.use("window-halves")
window_halves.setup({ chord = "SUPER + CTRL + " })
```

`om.use(name, url)` also installs on first use, so a rule file can carry
its own dependency. Saving anything under `lib/` reloads the rules like any
other file; a plugin that goes missing fails the rule that uses it, with a
notification. The copy of a built-in plugin under `lib/` is yours: read it,
change it (`om plugin list` then says `built-in, changed`), or remove it and
add it again for a fresh one.

To write one:

```sh
om plugin new my-plugin     # lib/my-plugin: init.lua, README.md, git init, rules.d/my-plugin.lua, then $EDITOR
```

The generated `init.lua` is a `setup(opts)` that binds one hotkey; change
it and it reloads on every save. When it is ready, push the repository to
GitHub and others install it with `om plugin add you/my-plugin`.

A plugin is code that runs as you, with everything `om.*` can do. Install
plugins from people you trust, as you would a Hammerspoon Spoon.

## API reference

### Triggers

Trigger functions register a handler and return a handle. `handle:remove()`
unregisters it.

```lua
-- A key chord. Both spellings work: Hyprland 0.56's "SUPER + ALT + J" and
-- the older "SUPER ALT, J". See "Hotkeys and your Hyprland config" above.
om.hotkey("SUPER + ALT + J", function()
  om.notify("pressed", "SUPER+ALT+J")
end)

-- The same, only while a window matching the pattern (or {class=, title=})
-- has focus: the chord is bound when such a window gets focus and unbound
-- when it loses it, so every other app keeps its own CTRL+S, natively.
om.app_hotkey("^firefox$", "CTRL + S", function()
  om.notify("firefox", "saved, the omaestro way")
end)

-- A window matching the patterns got focus (or lost it, with om.on_blur;
-- appeared, om.on_open; went away, om.on_close; changed title, om.on_title).
-- Matchers are Lua patterns on class and title; both must match; an empty
-- table matches every window. The handler gets a window object: the facts the
-- event carried (class, title, address, workspace when known) plus the
-- methods of om.window(); `win:refresh()` fetches the rest.
om.on_focus({class = "^firefox$"}, function(win)
  om.log("now in", win.title)
end)
om.on_blur({title = "YouTube"}, function(win)
  om.notify("Back to work", win.title)
end)
om.on_open({class = "^[Ss]potify$"}, function(win)
  win:to_workspace(9)
end)

-- The active workspace changed: {id, name}. A monitor was added, removed or
-- focused: {name, change}.
om.on_workspace(function(ws) om.log("workspace", ws.name) end)
om.on_monitor(function(mon) om.log("monitor", mon.name, mon.change) end)

-- The clipboard changed: the handler gets its text ("" for an image). Your
-- own om.paste and om.set_clipboard count as changes too.
om.on_clipboard(function(text) om.log("copied", #text, "chars") end)

-- The machine: sleep and wake (logind), USB devices (udev), the battery
-- (polled every 30 s), the network (NetworkManager). A source runs only
-- while a rule listens.
om.on_sleep(function() om.log("sleeping") end)
om.on_wake(function() om.notify("Welcome back") end)
om.on_usb(function(dev) om.log(dev.action, dev.device) end)        -- {action, device}
om.on_battery(function(b) om.log(b.percent, b.status) end)         -- {percent, status}
om.on_network(function(n) om.log(n.line) end)                      -- {line}: nmcli's words

-- A file or directory (recursively) changed: {path, kind} with kind
-- "create", "modify" or "remove". Editor swap files and backups are skipped.
om.on_file(os.getenv("HOME") .. "/Downloads", function(change)
  om.notify(change.kind, change.path)
end)

-- Every interval: "30s", "5m", "1h", or a sum like "1h30m". The first run is
-- one interval after the rules load. A tick whose previous run is still
-- going is skipped, not queued.
om.every("45m", function()
  om.notify("Stretch", "Stand up for a minute")
end)
-- A mode: the chord enters it, then single keys run handlers until Escape
-- (or any opts.exit key). opts.hint shows as a notification on entry;
-- opts.once leaves after the first key. Keys are chords ("h", "SHIFT + h").
om.mode("SUPER + ALT + W", {
  h = function() om.window():place("left") end,
  l = function() om.window():place("right") end,
}, { hint = "Window mode: h l, Esc", exit = {"q"} })

-- Once, after a delay; and every day at a clock time. Both handles have :cancel().
local timer = om.after("10m", function() om.notify("Tea", "is ready") end)
om.at("17:30", function() om.notify("Wrap up", "Half an hour left") end)

-- Typed text: when the last keys typed anywhere spell the text, it is erased
-- from the window and the handler runs (a text expander). Needs the `input`
-- group, see "Typed triggers" below.
om.on_typed(":sig", function()
  om.type("Best regards,\nIlya")
end)

-- A named entry point, fired with `om trigger cleanup`.
local handle = om.trigger("cleanup", function()
  om.notify("cleanup", "done")
end)
handle:remove()
```

`om.on_typed(":sig", fn)` is reserved for typed triggers, which arrive in v2.
Calling it today raises an error.

### Actions

Actions return their result directly; none of them takes a callback. One that
fails raises a Lua error, which ends the handler and shows up as a
notification unless the rule catches it with `pcall`.

```lua
local text = om.selection()               -- the selected text (primary selection), "" if none
local copied = om.clipboard()             -- the clipboard as text, "" if empty or not text
om.set_clipboard("copied by a rule")

local out = om.shell("git -C ~/notes status --short")  -- stdout without the trailing newline;
                                                      -- raises with the exit code and stderr on failure
local answer = om.prompt("Search")        -- a line typed into a menu, or nil when cancelled
local pick = om.choose("Format", {"jpg", "png"})  -- one of the options, or nil
local out = om.shell("wc -l", {stdin = text, timeout = 5})  -- stdin and a timeout in seconds

om.store.set("count", om.store.get("count", 0) + 1)  -- values that survive reloads and restarts
local everything = om.store.all()         -- the whole store as a table; om.store.path is the file
om.json.encode({a = 1}, {pretty = true})  -- and back with om.json.decode(text)

local r = om.http("https://api.example.com/items", {json = {name = "x"}, headers = {authorization = "Bearer t"}})
-- {status = 201, ok = true, body = "...", headers = {...}, json = <decoded when the answer is JSON>}
-- opts: method (default GET, POST when json is a table), body, headers, timeout (seconds), json

local answer = om.llm("Summarize: " .. text)                     -- ask the configured model
local short = om.llm(text, {system = "Reply in one sentence.", model = "qwen3"})

om.paste(answer)                          -- put text at the cursor, replacing a selection
om.type(os.date("%Y-%m-%d"))              -- type text as keystrokes
om.key("ctrl+shift+t")                    -- press a chord in the focused window

om.dispatch("hl.dsp.window.float()")      -- a Hyprland dispatcher, as in hyprctl dispatch

om.notify("Build finished", "took 42 s")  -- desktop notification; the body is optional
local pick = om.notify("Deploy?", "to prod", {actions = {yes = "Go", no = "Wait"}, timeout = 30})
                                          -- with buttons it waits and returns the pressed key, or nil
om.spawn("uwsm-app -- mpv ~/clip.mp4")    -- start a command in the background, returns its pid
om.log("focus changed", 3, true)          -- a line in the daemon's log
```

`om.paste` works through the clipboard: it copies the text, has Hyprland
press the paste chord for the focused app (see Settings), and puts your
previous clipboard content back.

The selection can be stale. Text you selected a while ago in another window
is still the selection; a rule that replaces text acts on whatever window has
focus now.

`om.key` and `om.type` are pressed by Hyprland with your real keymap, so
every app takes them. `om.type` knows the keys of a US layout; a character
that has no key there (`ü`, `✓`) goes through a virtual keyboard instead,
which some setups (an input method such as fcitx5) mangle. For anything
long or non-ASCII, `om.paste` is the sturdier route.

In a handler started by a hotkey, the first `om.type`, `om.key` or `om.paste`
waits until 400 ms after the press. Keys injected while SUPER or ALT are
still held would arrive as shortcuts, and Hyprland cannot report when they
are released.

`om.dispatch` takes what `hyprctl dispatch` takes on Hyprland 0.56: a Lua
dispatcher expression such as `hl.dsp.window.center()`. The list is in
`/usr/share/hypr/stubs/hl.meta.lua` under `HL.DspNamespace`.

`om.shell` runs the command through `sh -c` and waits for it. `om.prompt`
uses Omarchy's menu (`omarchy-menu-input`) when present, else walker, wofi,
fuzzel or rofi, else the `prompt_command` setting; with none of those it
raises an error saying so.

### Apps

```lua
om.launch("uwsm-app -- firefox")          -- start a program in the session (through Hyprland)
local win = om.focus("^firefox$")         -- focus the first window matching a class pattern, or nil
local win = om.focus({title = "Inbox"}, "uwsm-app -- thunderbird")  -- focus it, or launch it and
                                          -- wait up to 15 s for its window; nil if none came
for _, app in ipairs(om.apps()) do        -- running apps: {class, count, windows}
  om.log(app.class, app.count)
end
```

### Windows, monitors, workspaces

```lua
local win = om.window()                   -- the focused window, or nil
-- Facts: address, class, title, initial_class, workspace, workspace_id, monitor (id),
-- x, y, width, height, floating, fullscreen_mode ("none", "maximized", "fullscreen"),
-- pinned, pid, xwayland, focused.
win:place("left")                         -- float it on the left half of its monitor
win:place({x = 0.25, y = 0, w = 0.5, h = 1}) -- or on any fraction of the usable area
win:move(100, 200) win:resize(1200, 800)  -- exact logical pixels (floating windows)
win:center() win:float(true) win:pin() win:fullscreen("maximized")
win:focus() win:close() win:to_workspace(3) win:to_workspace("special:scratch", true)

for _, w in ipairs(om.windows({class = "^firefox$", workspace = 2})) do  -- every window, filtered
  w:to_workspace(1)                       -- filters: class, title (patterns), workspace, monitor
end

om.layout({                               -- arrange the desk in one call; returns how many
  {class = "^firefox$", place = "left"},  -- windows were placed. Entries: class/title patterns,
  {class = "^code$", workspace = 2, place = "right", all = true},  -- workspace, place, all
})

local pos = om.mouse()                    -- {x, y}; om.mouse_to(x, y) moves the pointer

local m = om.monitor()                    -- the focused monitor, or om.monitor("DP-2")
-- {id, name, description, x, y, width, height, scale, transform, focused, workspace}
om.monitors()                             -- all of them; width and height are logical pixels
om.workspace()                            -- the active one: {id, name, monitor, windows, has_fullscreen}
om.workspaces()
```

Placement names: `left`, `right`, `top`, `bottom`, `top-left`, `top-right`,
`bottom-left`, `bottom-right`, `left-third`, `middle-third`, `right-third`,
`left-two-thirds`, `right-two-thirds`, `center` (keeps the size), `max`.
They use the monitor's area minus what bars reserve. A window's facts are a
snapshot from when the table was made; call `om.window()` again for fresh
ones.

### Modules

`require("name")` finds `~/.config/omaestro/lib/name.lua` (or
`lib/name/init.lua`). `om.use("name", "https://github.com/x/name")` requires
it too, and first clones the repository into `lib/name` when it is not
there (`"x/name"` is short for GitHub). Saving anything under `lib/` reloads
the rules like any other file. See "Plugins" above for the convention and
the `om plugin` commands.

### String helpers

Available as methods on every string:

```lua
("  padded  "):trim()            -- "padded"
("a,b,c"):split(",")             -- {"a", "b", "c"}; a plain separator, not a pattern
("one  two"):split()             -- {"one", "two"}; no separator splits on whitespace
("rules.d/x.lua"):starts_with("rules.d/")  -- true
("rules.d/x.lua"):ends_with(".lua")        -- true
```

## The `om` command

```
om daemon [--foreground] [--config-dir DIR]   run the daemon
om status                                     what the daemon is doing, and who keeps it running
om start                                      start the daemon through its systemd user unit
om stop                                       stop it (the unit's, or one started by hand)
om restart                                    stop it and start it again, after a new build say
om list                                       registered triggers and where they were defined
om list --json                                the same as one JSON array (what the panel reads)
om disable ID                                 switch a rule off; kept across reloads and restarts
om enable ID                                  switch it back on
om override [on|off]                          let rules take chords Hyprland already has, or give them back; alone: which it is
om status --json                              the status as JSON
om plugin available                           the plugins that ship with om, and which are installed
om plugin add NAME|URL [--ref TAG] [--no-rule] install one into ~/.config/omaestro/lib and write rules.d/NAME.lua
om plugin list [--json]                       installed plugins: name, version, Lua path, source
om plugin new NAME [--no-edit] [--no-rule]    start a plugin of your own and open it in $EDITOR
om plugin update [NAME]                       git pull the plugins that came from a repository
om plugin remove NAME [--force]               delete one and the rule it wrote (refuses to lose uncommitted work)
om trigger NAME                               fire a named trigger
om reload                                     reload the rule files now
om eval 'return 1+1'                          run Lua inside the daemon, print the result
om repl                                       the same, line by line (Ctrl+D to leave)
om doctor                                     check the session, the tools, and leftover binds
om panel                                      open or close the rules panel of the Omarchy plugin
om doctor --clear                             also remove binds an earlier daemon left behind
```

`om eval` runs in the same state as your rules, so it can inspect them:
`om eval 'return some_global'`.

Every command takes `--socket PATH` (or `OMAESTRO_SOCKET`) to talk to a daemon
on a non-default socket; `om daemon` also takes `OMAESTRO_CONFIG_DIR`. The
default socket is `$XDG_RUNTIME_DIR/omaestro.sock`. It is readable and
writable only by you: anyone who can write to it can run code as you.

## Logs

```sh
journalctl --user -u omaestro -f
```

`OMAESTRO_LOG=debug` (or `warn`, `error`, `trace`) changes the level.

## Development

```sh
make check                                   # fmt, clippy and the tests; no display or Hyprland needed
make smoke                                   # live checks; run inside the Hyprland session
make smoke-press                             # the same, plus a hotkey you press by hand
make plugin                                  # link this checkout into the Omarchy shell as the plugin, enabled
make plugin-reload                           # after editing Panel.qml or Service.qml: restarts the shell so it re-reads them
make plugin-remove                           # disable it and remove the link
make help                                    # the other targets: install, uninstall, dist, release
```

The Makefile only names the tasks; the work is in `cargo` and `scripts/`.

`make plugin` is how to try `Panel.qml` and `Service.qml` from a checkout:
the shell loads them through a symlink in `~/.config/omarchy/plugins/`. The
shell's file watch does not follow the link, and the plugin is `keepLoaded`
(so that its service, and a daemon it supervises, survive plugin hot
reloads), so after editing them run `make plugin-reload`, which restarts the
shell. With `om` on PATH (`make install`) the plugin's service
runs that binary instead of downloading a release, and if the systemd unit's
daemon is already running it leaves it alone and just checks again every
minute. Open the panel with `omarchy-shell shell toggle
io.github.iluxav.omaestro`.

`scripts/smoke.sh` does not change your Hyprland. Everything that binds keys,
injects keystrokes or uses the clipboard runs inside a nested Hyprland: a
second compositor in a window, with a scratch config. It also uses a
temporary config and socket, so `~/.config/omaestro` and a daemon that is
already running are left alone. You will see a window open for half a minute
and a few notifications.

## License

MIT
