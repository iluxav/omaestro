# tile-close-undo

SUPER+W closes the focused window three seconds late, and SUPER+Z in the
meantime brings it back as it was. Until then the window only waits,
hidden on a special workspace: the app keeps running, so its content,
scroll position and unsaved text are all still there when it returns.
Reopening the app could not do that.

| Chord | What |
|---|---|
| SUPER+W | hide the window; it closes for real after 3 s |
| SUPER+Z | bring back the window closed last (again for the one before) |

## SUPER+W is Omarchy's

Omarchy binds SUPER+W to "Close window", and omaestro does not take a
chord that is already bound: without one of these the rule shows as
refused in the panel and in `om list`.

- Remove Omarchy's bind, in `~/.config/hypr/bindings.lua`:

  ```lua
  hl.unbind("SUPER + W")
  ```

- Or turn override on (`om override on`, or the switch at the bottom of
  the panel). It lets every rule take a chord that is already bound, and
  Omarchy's SUPER+W comes back when this rule is switched off or removed.

## Install

```sh
om plugin add https://github.com/iluxav/omaestro/tree/main/plugins/tile-close-undo
```

It installs with its defaults and lists them; `--set key=value` after
the URL (once per option) installs it with yours instead, and
`om plugin configure tile-close-undo` opens them as a form in your editor.
Either way they are written to `~/.config/omaestro/rules.d/tile-close-undo.lua`,
which you can also edit.

Options go in `~/.config/omaestro/rules.d/tile-close-undo.lua`:

```lua
om.use("tile-close-undo").setup({
  delay = 5,
  notify = true,
})
```

## Options

- `chord`: the close chord (default `SUPER + W`).
- `undo`: the chord that brings the last closed window back (default
  `SUPER + Z`).
- `delay`: seconds before the window really closes (default 3).
- `notify`: a notification on each close that names the undo chord
  (default `false`).

## Good to know

- The window closes the way SUPER+W always closed it: the app is asked to
  quit. One that asks first ("Save changes?") stays open; five seconds
  later it is brought back where it was, with a notification.
- A tiled window comes back to its workspace, but the layout places it
  next to the focused window, not always in its old spot. A floating
  window comes back where it was. A window taken out of a group comes back
  on its own.
- While it waits the app still runs: a video keeps playing for those
  seconds.
- A reload or a restart of omaestro while a window waits puts the window
  back instead of closing it.
