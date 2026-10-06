# night-light

Makes the screen warmer at night and normal again in the morning: 4000 K
from 20:00, hyprsunset's plain colors from 07:00. SUPER+ALT+S switches
now, until the next scheduled switch (back from a bright room at 22:00,
or warm early). While GIMP, Inkscape or darktable has focus the screen
shows true colors, and goes warm again when you leave it. After a wake
from sleep the screen is set for the time of day, so a laptop closed at
19:00 and opened at 21:00 is warm.

It drives hyprsunset, which Omarchy ships, through `hyprctl hyprsunset`,
and starts it when it is not running.

| Chord | What |
|---|---|
| SUPER+ALT+S | warm or normal now; a notification says which |

## Options

```lua
-- rules.d/night-light.lua
om.use("night-light").setup({
  warm_at = "20:00",            -- false: no schedule, only the hotkey
  normal_at = "07:00",
  temperature = 4000,           -- 6000 is hyprsunset's day
  toggle = "SUPER + ALT + S",   -- false: no hotkey
  true_colors = { "^[Gg]imp$", "^org%.inkscape%.Inkscape$", "^darktable$" },
  notify = false,               -- true: a notification on each scheduled switch
})
```

`true_colors` is a list of window class patterns (Lua patterns); the
`window-rules` plugin's `log_focus` shows an app's class in the journal.
An empty list switches it off.

## hyprsunset's own schedule

hyprsunset reads `~/.config/hypr/hyprsunset.conf` and switches at the
times of its profiles too. Omarchy's default there is one profile, plain
colors at 07:00, which agrees with `normal_at`. If you add a 20:00
profile with a temperature there as its comment suggests, keep one
schedule, not both: either that file or this plugin.

## Install

```sh
om plugin add https://github.com/iluxav/omaestro/tree/main/plugins/night-light
```

It installs with its defaults and lists them; `--set key=value` after
the URL (once per option) installs it with yours instead, and
`om plugin configure night-light` opens them as a form in your editor.
Either way they are written to `~/.config/omaestro/rules.d/night-light.lua`,
which you can also edit.
