# apps

Bring an app to the front or start it, and arrange a desk with one chord.

| Chord | What |
|---|---|
| SUPER+ALT+B | Firefox to the front, started if it is not running |
| SUPER+ALT+L | browser on the left half, editor on the right, terminal on workspace 2 filling its screen |

## Install

```sh
om plugin add https://github.com/iluxav/omaestro/tree/main/plugins/apps
```

Its options are Lua tables, so they go in
`~/.config/omaestro/rules.d/apps.lua` by hand (examples below).

Options go in the rule that writes, `~/.config/omaestro/rules.d/apps.lua`:

```lua
om.use("apps").setup({
  focus = {
    ["SUPER + ALT + B"] = { class = "^firefox$", command = "uwsm-app -- firefox" },
    ["SUPER + ALT + E"] = { class = "^code$", command = "uwsm-app -- code" },
  },
  layouts = {
    ["SUPER + ALT + L"] = {
      { class = "^firefox$", place = "left" },
      { class = "^code$", place = "right" },
    },
  },
})
```

## Options

- `focus`: chord → `{ class =, title =, command = }`. Class and title are Lua
  patterns; `command` starts the app when no window matches, and the plugin
  waits up to 15 seconds for its window. `uwsm-app --` runs it in its own
  systemd scope, as Omarchy does.
- `layouts`: chord → a list of `om.layout` entries: matchers, an optional
  `workspace`, and a `place` (`left`, `right`, `top`, `bottom`, corners,
  thirds, `center`, `max`, or `{x=, y=, w=, h=}` fractions). `all = true`
  places every matching window instead of the first.
- `app_hotkeys`: app (class pattern, or `{class=, title=}`) → chord →
  handler. Bound only while that app has focus, so every other app keeps
  the chord:
  ```lua
  app_hotkeys = {
    ["^firefox$"] = { ["CTRL + SHIFT + D"] = function() om.notify("firefox", "hello") end },
  },
  ```

Find an app's class with `om eval 'return om.window().class'` while it is
focused.
