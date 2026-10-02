# panel

SUPER+ALT+O opens and closes the rules panel of the Omarchy plugin: every
rule with a switch, the override switch, and a reload button.

A config omaestro creates now has this in its `init.lua` already
(`om.hotkey("SUPER + ALT + O", om.panel)`), and the omaestro icon in the bar
opens the panel too. This plugin is for configs from before that; do not
use both, a chord takes one rule.

The panel exists when omaestro is installed as the Omarchy plugin
(`omarchy plugin add ...`). The same command, `omarchy-shell shell toggle
io.github.iluxav.omaestro`, works from a bind in `~/.config/hypr/bindings.lua`
too, which also opens the panel when the daemon is down.

## Install

```sh
om plugin add panel
```

It installs with its defaults and lists them; `om plugin configure panel`
opens them as a form in your editor. Either way they are written to
`~/.config/omaestro/rules.d/panel.lua`, which you can also edit.

## Options

- `chord`: the hotkey (default `SUPER + ALT + O`), in
  `~/.config/omaestro/rules.d/panel.lua`.
