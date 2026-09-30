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

The v1 API is complete: hotkeys, focus triggers, timers, named triggers,
the selection and clipboard, the local model, paste, typing, key presses,
shell commands, a prompt and Hyprland dispatchers. Typed triggers
(`om.on_typed`) and packaging as an Omarchy plugin are next; see `AGENTS.md`
for the milestones.

## Requirements

- Omarchy (or any Arch setup) with Hyprland 0.56 or newer, on Wayland.
- `hyprctl`, `wtype`, `wl-copy`, `wl-paste` and `notify-send` on PATH (all
  present on Omarchy). `om doctor` checks them.
- For `om.llm`: [Ollama](https://ollama.com) running locally with a model
  pulled, or any OpenAI-compatible endpoint.
- A Rust toolchain to build.

## Install

```sh
cargo install --path .
install -Dm644 systemd/omaestro.service ~/.config/systemd/user/omaestro.service
systemctl --user enable --now omaestro
```

`cargo install` puts `om` in `~/.cargo/bin`, which is where the unit expects
it. Check that everything is in place:

```sh
om doctor
om status
```

To run it in a terminal instead of systemd: `om daemon --foreground`.

The daemon must be started inside the Hyprland session. If
`HYPRLAND_INSTANCE_SIGNATURE` is unset it refuses to start and says so.

## Remove

```sh
systemctl --user disable --now omaestro
rm ~/.config/systemd/user/omaestro.service
cargo uninstall omaestro
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

More recipes are in `examples/`, each a file you can drop into `rules.d/`:

| File | What it does |
|---|---|
| `rewrite-selection.lua` | SUPER+ALT+J rewrites the selection with the local model (the rule from the top of this page) |
| `summarize-selection.lua` | SUPER+ALT+M shows a three-sentence summary of the selection |
| `translate-selection.lua` | SUPER+ALT+T replaces the selection with its translation |
| `selection-upper.lua` | SUPER+ALT+U upper-cases the selection, no model involved |
| `type-date.lua` | SUPER+ALT+D types today's date |
| `clipboard-notes.lua` | SUPER+ALT+N appends the clipboard to `~/notes/clips.md` |
| `prompt-search.lua` | SUPER+ALT+S asks for a query and opens it in the browser |
| `window-center.lua` | SUPER+ALT+C floats and centers the window, or tiles it back |
| `float-on-focus.lua` | floats and centers Calculator when it opens |
| `focus-log.lua` | logs every focus change to the journal |
| `every-stretch.lua` | a reminder every 45 minutes |
| `named-trigger.lua` | a trigger fired from the shell with `om trigger hello` |

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

## Hotkeys and your Hyprland config

omaestro never writes to `~/.config/hypr`. `om.hotkey` registers the bind in
the running Hyprland, and it is removed again when the rule goes away, on
reload and when the daemon stops. If Hyprland reloads its own config, the
binds are put back.

A chord that Hyprland already has a bind for is refused, with a notification
naming the existing bind. Hyprland can only remove binds by chord, so sharing
one would mean that removing ours removes yours. To use such a chord, free
it in your Hyprland config first. On Omarchy, `SUPER + J` is "Toggle window
split"; to give it to a rule, add this to `~/.config/hypr/bindings.lua`:

```lua
hl.unbind("SUPER + J")
```

Your hotkeys show up in Hyprland's bind list (and Omarchy's keybindings menu)
with a description like `omaestro: rules.d/rewrite.lua:3`.

Run one daemon per Hyprland session.

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
```

Terminals (Alacritty, kitty, foot, Ghostty, WezTerm) are already known to
paste with `ctrl+shift+v`. Saving the file reloads it; a mistake in it is
reported like a rule error and the previous settings stay.

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

-- A window matching the patterns got focus (or lost it, with om.on_blur).
-- Matchers are Lua patterns on class and title; both must match; an empty
-- table matches every window. The handler gets {class, title, address}.
om.on_focus({class = "^firefox$"}, function(win)
  om.log("now in", win.title)
end)
om.on_blur({title = "YouTube"}, function(win)
  om.notify("Back to work", win.title)
end)

-- Every interval: "30s", "5m", "1h", or a sum like "1h30m". The first run is
-- one interval after the rules load. A tick whose previous run is still
-- going is skipped, not queued.
om.every("45m", function()
  om.notify("Stretch", "Stand up for a minute")
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

local answer = om.llm("Summarize: " .. text)                     -- ask the configured model
local short = om.llm(text, {system = "Reply in one sentence.", model = "qwen3"})

om.paste(answer)                          -- put text at the cursor, replacing a selection
om.type(os.date("%Y-%m-%d"))              -- type text as keystrokes
om.key("ctrl+shift+t")                    -- press a chord in the focused window

local win = om.window()                   -- the focused window, or nil
-- {class = "firefox", title = "...", address = "0x...", workspace = "2", floating = false}
om.dispatch("hl.dsp.window.float()")      -- a Hyprland dispatcher, as in hyprctl dispatch

om.notify("Build finished", "took 42 s")  -- desktop notification; the body is optional
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
om status                                     what the daemon is doing
om list                                       registered triggers and where they were defined
om trigger NAME                               fire a named trigger
om reload                                     reload the rule files now
om eval 'return 1+1'                          run Lua inside the daemon, print the result
om doctor                                     check the session, the tools, and leftover binds
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
cargo test                                   # no display or Hyprland needed
cargo clippy --all-targets -- -D warnings
scripts/smoke.sh                             # live checks; run inside the Hyprland session
scripts/smoke.sh --press                     # the same, plus a hotkey you press by hand
```

`scripts/smoke.sh` does not change your Hyprland. Everything that binds keys,
injects keystrokes or uses the clipboard runs inside a nested Hyprland: a
second compositor in a window, with a scratch config. It also uses a
temporary config and socket, so `~/.config/omaestro` and a daemon that is
already running are left alone. You will see a window open for half a minute
and a few notifications.

## License

MIT
