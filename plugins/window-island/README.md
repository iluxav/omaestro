# window-island

Keep a Chrome video, chat or another window on the left of a monitor across
workspaces. Normal tiles leave space beside it instead of extending under it.
One window can be docked per monitor. Width controls resize the island and
the space left for the other tiles together.

| Chord | Action |
|---|---|
| SUPER+CTRL+I | Dock the focused window; on the island window, undock it |
| SUPER+CTRL+[ | Narrow the island on the focused monitor |
| SUPER+CTRL+] | Widen the island on the focused monitor |

Focus Chrome, dock it, then switch workspaces normally. Docking a different
window on the same monitor restores the old one first. Closing the island
window releases its space. Undocking restores its original floating/pinned
state and, if it floated, its original size and position. A tiled window
rejoins the current workspace's layout; its old split position is not saved.

## Install

From a checkout, including changes that have not been published yet:

```sh
om plugin add ./plugins/window-island
```

After this plugin has been published to the repository:

```sh
om plugin add https://github.com/iluxav/omaestro/tree/main/plugins/window-island
```

`om plugin configure window-island` opens the settings form. Options live in
`~/.config/omaestro/rules.d/window-island.lua` and can also be edited there:

```lua
om.use("window-island").setup({
  width = 800,
  step = 80,
  min_width = 240,
  min_remaining = 480,
  gap = 10,
  toggle = "SUPER + CTRL + I",
  narrower = "SUPER + CTRL + bracketleft",
  wider = "SUPER + CTRL + bracketright",
})
```

Each chord accepts `false` to disable it. Chords already taken by Hyprland
are refused by omaestro as usual; choose free ones in the form. All sizes
are logical pixels: on a 5120-pixel-wide display at scale 1.6, an 800-wide
island takes a quarter of the screen. `width` is the initial width when a
window is docked; hotkey adjustments survive rule reloads and daemon restarts
within the same Hyprland session. The island is clamped to leave at least
`min_remaining` usable pixels for normal tiles. Too-small monitors are
refused. Bars, display scale, rotation and monitor offsets are accounted for.

## Behavior and cleanup

The window floats and is pinned internally. A temporary Hyprland workspace
rule reserves its column using outer gaps, so normal tiles do not overlap
it. No files under `~/.config/hypr` are written and no display mode, scale
or bar setting is changed. Remove any manually configured island reservation
before using this plugin, so the two reservations do not add together.

The plugin owns the workspace selector `s[false] m[MONITOR]` for each active
island monitor; do not declare an identical selector yourself. During docking,
normal workspaces on that monitor use the global outer gaps plus the island
width. Other workspace settings remain in effect. Undocking disables that
temporary rule, restoring the underlying workspace gap rules.

Rules reloads and daemon restarts restore active islands. A Hyprland config
reload drops the temporary rules; the next heartbeat restores them within
two seconds. State from an old compositor session is discarded. Closing the
window or unplugging its monitor releases the island. If the plugin is
removed, its maintenance rule disabled, or the daemon stops, a compositor
timer releases the column and restores the window within ten seconds.

To remove it, undock first for immediate cleanup, then:

```sh
om plugin remove window-island
```

This version uses hotkeys to dock and change width. It has no draggable
divider or drag-to-dock behavior. Special workspaces and fullscreen windows
cannot be docked. Fullscreen windows and unrelated floating windows can
cover the island; its column is reserved for normal tiling only.

## Verify in a test session

Install into a scratch omaestro configuration in a nested Hyprland, as in
`scripts/smoke.sh`; keep `HYPRLAND_NO_SD_VARS=1`. Dock a scratch terminal,
open another tile and switch workspaces: the terminal stays on the left,
and the other tile starts beyond the column. Check width controls, undock,
close, rule reload, compositor reload and daemon exit. Confirm the gap
disappears and the window's prior state returns. This needs Hyprland 0.56's
Lua API (`hl.workspace_rule`, window dispatchers and `hl.timer`).
