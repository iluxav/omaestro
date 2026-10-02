# window-halves

The focused window on a half, a third or a corner of its monitor with one
chord, the way Rectangle does it on macOS. The window floats and takes the
exact area; the bar's space is left alone.

| Chord | Where |
|---|---|
| CTRL+ALT+Left / Right | the left / right half |
| CTRL+ALT+Up | the whole screen |
| CTRL+ALT+Down | centered |
| SUPER+ALT+C | float and center, or tile back |

Omarchy binds every SUPER+arrow combination itself (focus, swap, groups,
workspace to monitor), which is why the arrows go with CTRL+ALT. VS Code
uses Ctrl+Alt+Up/Down for "add cursor"; a Hyprland bind wins over the app,
so change `chord` if you need that.

## Install

```sh
om plugin add https://github.com/iluxav/omaestro/tree/main/plugins/window-halves
```

It installs with its defaults and lists them; `--set key=value` after
the URL (once per option) installs it with yours instead, and
`om plugin configure window-halves` opens them as a form in your editor.
Either way they are written to `~/.config/omaestro/rules.d/window-halves.lua`,
which you can also edit.

Options go in `~/.config/omaestro/rules.d/window-halves.lua`:

```lua
om.use("window-halves").setup({
  chord = "SUPER + CTRL + ",
  keys = { Left = "left-third", Right = "right-two-thirds", Up = "top", Down = "bottom" },
})
```

## Options

- `chord`: the modifiers in front of each key (default `CTRL + ALT + `).
- `keys`: key → place. Places: `left`, `right`, `top`, `bottom`,
  `top-left`, `top-right`, `bottom-left`, `bottom-right`, `left-third`,
  `middle-third`, `right-third`, `left-two-thirds`, `right-two-thirds`,
  `center`, `max`, or fractions `{x = 0, y = 0, w = 0.5, h = 1}`.
- `toggle`: the float-and-center chord (`false`: off).
