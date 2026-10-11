# clock

Tap SUPER twice on its own, quickly (each press let go within 0.3 s, the
second within 0.3 s of the first), and the time, the date and the weather
show in large type in the middle of the screen for a second. A single tap
does nothing. SUPER+ALT+A does the same:

```
     14:32
Saturday, October 11
   Sunny, +22°C
```

The card sits over every window on the monitor you are using, takes no
keyboard focus and lets clicks through. With omaestro installed as an
Omarchy plugin the Omarchy shell draws it in your theme; elsewhere omaestro
draws it through quickshell, and without quickshell it is a notification.

The weather comes from [wttr.in](https://wttr.in), fetched every 15 minutes
in the background so the card shows at once. Until the first answer comes
(right after a start, or when the network is down) the card shows the time
and the date, and the press fetches the weather and adds it while the card
is still up. Your location is sent to wttr.in, or, with no `location`, your
IP address tells it where you are.

SUPER keeps its job: apps still get it, every shortcut with it works as
before, and a press that ran a shortcut (SUPER+J) never shows the clock,
however quick, and starts the count over. A SUPER held longer than
`within` does not count either. A quick SUPER with a key that is no
shortcut, or with a mouse click, does count as a tap.

| Key | What |
|---|---|
| SUPER, tapped twice alone | the time, the date and the weather, for a second |
| SUPER+ALT+A | the same |

## Options

```lua
-- rules.d/clock.lua
om.use("clock").setup({
  tap = "SUPER",            -- SUPER_R, ALT, CTRL, SHIFT; "off": no tap
  taps = 2,                 -- 1: a single tap is enough
  within = 0.3,             -- seconds the key may be down, and between the taps
  chord = "SUPER + ALT + A",  -- false: no chord
  seconds = 1,              -- how long the card stays up
  time = "%H:%M",           -- "%I:%M %p" for 12-hour
  date = "%A, %B %e",       -- false: no date
  weather = true,           -- false: no weather, nothing sent to wttr.in
  location = "Lisbon",      -- a city or "lat,lon"; leave it out to go by your IP
  units = "auto",           -- "metric", "imperial"; auto is wttr.in's pick for the place
  refresh = "15m",          -- how often the weather is fetched
  position = "center",      -- "top" or "bottom"
})
```

`time` and `date` are `os.date` formats (Lua's, which are C's `strftime`
without the `%-d` style flags; `%e` is the day without a leading zero).

## Install

```sh
om plugin add https://github.com/iluxav/omaestro/tree/main/plugins/clock
```

It installs with its defaults and lists them; `--set key=value` after
the URL (once per option) installs it with yours instead, and
`om plugin configure clock` opens them as a form in your editor.
Either way they are written to `~/.config/omaestro/rules.d/clock.lua`,
which you can also edit.
