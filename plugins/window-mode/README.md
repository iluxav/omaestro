# window-mode

SUPER+ALT+W enters a window mode: single keys place the focused window
until Escape (or q) leaves it. A hint shows as a notification on entry.

| Key | Where |
|---|---|
| h j k l | left, bottom, top, right half |
| H L | left, right third |
| c | centered |
| m | the whole screen |
| f | float / tile |
| Esc, q | leave the mode |

Modes are Hyprland submaps: the keys show in `hyprctl binds` and Omarchy's
keybindings menu like any other bind.

## Install

```sh
om plugin add https://github.com/iluxav/omaestro/tree/main/plugins/window-mode
```

It installs with its defaults and lists them; `--set key=value` after
the URL (once per option) installs it with yours instead, and
`om plugin configure window-mode` opens them as a form in your editor.
Either way they are written to `~/.config/omaestro/rules.d/window-mode.lua`,
which you can also edit.

Options go in `~/.config/omaestro/rules.d/window-mode.lua`:

```lua
om.use("window-mode").setup({
  chord = "SUPER + ALT + W",
  keys = { h = "left", l = "right", f = "float", t = function() om.window():pin() end },
  once = true,
})
```

## Options

- `chord`: the entry chord.
- `keys`: key → a place name (`left`, `right`, `top`, `bottom`, corners,
  thirds, `center`, `max`), fractions `{x=, y=, w=, h=}`, `"float"`, or a
  function.
- `hint`: the notification on entry (`false`: none).
- `exit`: keys that leave the mode besides Escape (default `{ "q" }`).
- `once`: leave the mode after one key (default `false`).
