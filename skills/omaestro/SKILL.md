---
name: omaestro
description: Write and debug omaestro rules and plugins — Lua automation for Omarchy/Hyprland (hotkeys, app-scoped hotkeys, window rules, timers, clipboard, a local model). Use whenever the user mentions omaestro, `om.*`, ~/.config/omaestro, rules.d, or asks to automate something on their Hyprland desktop with a hotkey or a rule.
---

# omaestro

omaestro is a daemon (`om`) that loads Lua rule files and runs them against
the desktop: "when this hotkey / focus change / timer / clipboard change
happens, do these steps". Think Hammerspoon for Hyprland. This skill is the
map of its API and conventions; `om --help` and the README in the repository
have the rest.

## Where things live

```
~/.config/omaestro/init.lua          loaded first
~/.config/omaestro/rules.d/*.lua     loaded after it, in name order; one file per concern
~/.config/omaestro/lib/<name>/       plugins (init.lua returning a table); om.use(name) loads one
~/.config/omaestro/omaestro.toml     model endpoint, paste chords, prompt command (optional)
```

Saving any of these reloads the rules; nothing to restart. A file that
fails to load keeps the previous rules running and shows a notification
with `file:line: message`. A rule that errors never stops the daemon.

## Checking your work

```sh
om status                 # running? who supervises it? last load error?
om list                   # every trigger: id, kind, origin; "refused: ..." when a chord is taken
om trigger <id>           # fire a hotkey or named trigger without pressing it
om eval 'return om.window()'   # run Lua inside the daemon; inspect state
om reload                 # after an edit, if you do not want to wait for the watcher
journalctl --user -u omaestro -f
om doctor                 # tools, session, leftover binds
```

Prefer `om list` and `om trigger` to verify a rule; never test key
injection by typing into the terminal the daemon logs to.

## The API (global table `om`)

Triggers return a handle with `:remove()`.

```lua
om.hotkey("SUPER + ALT + J", fn)                 -- a chord; Hyprland runs the handler
om.app_hotkey("^firefox$", "CTRL + S", fn)       -- bound only while a matching window has focus;
                                                 -- matcher: class pattern or {class=, title=}
om.on_focus({class = "^firefox$"}, fn)           -- fn(win); also on_blur, on_open, on_close, on_title
om.on_workspace(fn)  om.on_monitor(fn)           -- fn({id, name}) / fn({name, change})
om.on_clipboard(fn)                              -- fn(text); "" for images
om.on_sleep(fn) om.on_wake(fn) om.on_usb(fn) om.on_battery(fn) om.on_network(fn)
om.on_file(path, fn)                             -- fn({path, kind}); kind: create/modify/remove
om.every("45m", fn)  om.after("10m", fn)  om.at("17:30", fn)   -- intervals: 30s, 5m, 1h, 1h30m
om.mode("SUPER + ALT + W", {h = fn, l = fn}, {hint = "...", exit = {"q"}, once = false})
om.menu("SUPER + ALT + P", {{"Label", function(ctx) end}, ...}, {title = "Do", selection = true})
                                                 -- a menu of actions; last pick first; ctx = {window, selection}
                                                 -- taken before the menu; items may be function(ctx) -> list
om.on_typed(":sig", fn)                          -- a text expander; needs the input group
om.trigger("name", fn)                           -- fired with `om trigger name`
```

Actions return values; none takes a callback. A failure is a Lua error.

```lua
om.selection()  om.clipboard()  om.set_clipboard(text)
om.paste(text)   -- the sturdy way to put text at the cursor (replaces a selection)
om.type(text)    -- keystrokes; short ASCII only, prefer paste otherwise
om.key("ctrl+shift+t")
om.llm(prompt, {system = "...", model = "..."})   -- Ollama/OpenAI-compatible, from omaestro.toml
om.shell("cmd", {stdin = text, timeout = 5})     -- stdout; raises on non-zero exit
om.spawn("uwsm-app -- firefox")                  -- background, returns the pid
om.notify(title, body, {actions = {yes = "Go"}, timeout = 30})   -- with actions: returns the key pressed
om.busy("Rewriting…")  -- a notification while the handler works; gone when it ends (om.busy() sooner)
om.prompt("label")  om.choose("label", {"a", "b"})               -- nil when cancelled
om.http(url, {json = {...}, headers = {...}, method = "POST"})   -- {status, ok, body, json, headers}
om.store.get(key, default)  om.store.set(key, value)             -- survives reloads and restarts
om.json.encode(v)  om.json.decode(text)
om.dispatch("hl.dsp.window.float()")             -- a Hyprland 0.56 Lua dispatcher expression
om.log(...)
```

Windows and apps:

```lua
local win = om.window()        -- focused window or nil: address, class, title, workspace, floating, x, y, width, height ...
win:place("left")              -- left, right, top, bottom, corners, thirds, center, max, or {x=, y=, w=, h=} fractions
win:move(x, y) win:resize(w, h) win:center() win:float(true) win:pin() win:fullscreen("maximized")
win:focus() win:close() win:to_workspace(3)
om.windows({class = "^code$", workspace = 2})   om.apps()
om.launch("uwsm-app -- firefox")   om.focus("^firefox$", "uwsm-app -- firefox")   -- focus or start
om.layout({{class = "^firefox$", place = "left"}, {class = "^code$", place = "right"}})
om.monitor()  om.monitors()  om.workspace()  om.workspaces()  om.mouse()  om.mouse_to(x, y)
```

String helpers on every string: `:trim()`, `:split(sep)`, `:starts_with(p)`, `:ends_with(p)`.

## Conventions that keep rules working

- Chords: Hyprland syntax `"SUPER + ALT + J"`. A chord Hyprland already
  has (Omarchy's own, or the user's `bindings.lua`) is refused with a
  notification, and `om list` shows `refused:`. Omarchy uses every
  SUPER+arrow combination, and SUPER+ALT with S, F, G, K, Space, Return,
  arrows and more. CTRL+ALT+arrows and SUPER+ALT+<letter> for most letters
  are free. `om override on` lets rules take chords anyway (ask the user
  before suggesting it).
- Matchers are Lua patterns: escape dots (`^org%.gnome%.Calculator$`).
  Find a class with `om eval 'return om.window().class'` while the app has
  focus.
- Use `om.paste` for text longer than a few ASCII characters or anything
  non-ASCII; `om.type` goes through a US keymap.
- `om.selection()` returns "" for an old selection: made in a window that
  no longer has focus, or already pasted/typed over by a rule. Check for ""
  and tell the user to select something; never fall back to the clipboard.
- Wrap slow work (a model call, a long command) in `om.busy("Doing X…")`
  so the user sees it is working; it closes itself when the handler ends.
- Handlers of one trigger run one at a time; a slow handler is logged, not
  killed. Keep handlers short; use `om.spawn` for long commands.
- `om.llm` sends text to the configured endpoint only (Ollama on localhost
  by default). Put instructions in `system`, the text alone in the prompt.

## Plugins

A plugin is a directory with `init.lua` returning a table, usually with
`setup(opts)`, under `~/.config/omaestro/lib/<name>/`. Rules load it:

```lua
local halves = om.use("window-halves")
halves.setup({ chord = "CTRL + ALT + " })
```

```sh
om plugin available            # omaestro's own (plugins/ in its repo): ai-text, window-halves, window-mode,
                               # window-rules, apps, text-tools, clipboard, reminders, web-search,
                               # system-events, downloads, panel
om plugin add window-halves    # fetched from GitHub into lib/, plus rules.d/window-halves.lua (options go there)
om plugin add you/repo         # any repository whose root is a plugin; you/repo/dir for one inside it;
                               # a GitHub browser URL, any git URL (--path, --ref), or ./a-directory
om plugin configure NAME       # its options as a form in $EDITOR (from its plugin.json); --set k=v without the form
om plugin new my-plugin        # a skeleton of your own (init.lua, README, plugin.json), git init, opened in $EDITOR
om plugin list | update | remove NAME | remove --all   # --all: every plugin and the rules om wrote, to start over
```

To write one: `om plugin new <name>`, keep every option in `setup(opts)`
with a default, use `false` for "switch this chord off", describe each simple
option in `plugin.json` (types: chord, modifiers + keys, string, path, bool,
number, interval, time, enum + options; `optional` allows none; `when`/`unless`
name a yes/no option it depends on) so `om
plugin configure` shows it in its form, document all options in the README, push to GitHub;
others install it with `om plugin add you/name`. A plugin's settings live in
the user's `rules.d/<name>.lua`, never in its code.

## Panel and lifecycle

`om panel` (or SUPER+ALT+O with the `panel` plugin) opens the Omarchy
panel: every rule with a switch, the override switch, a reload button.
`om disable <id>` / `om enable <id>` do the same from the terminal.
`om start`, `om stop`, `om restart` drive the daemon (systemd unit or the
Omarchy plugin's service).
