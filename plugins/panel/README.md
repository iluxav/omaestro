# panel

SUPER+ALT+O opens and closes the rules panel of the Omarchy plugin: every
rule with a switch, the override switch, and a reload button.

The panel exists when omaestro is installed as the Omarchy plugin
(`omarchy plugin add ...`). The same command, `omarchy-shell shell toggle
io.github.iluxav.omaestro`, works from a bind in `~/.config/hypr/bindings.lua`
too, which also opens the panel when the daemon is down.

## Install

```sh
om plugin add panel
```

It asks for its options as it installs (Enter keeps a default);
`om plugin configure panel` changes them later. Both write
`~/.config/omaestro/rules.d/panel.lua`, which you can also edit.

## Options

- `chord`: the hotkey (default `SUPER + ALT + O`), in
  `~/.config/omaestro/rules.d/panel.lua`.
