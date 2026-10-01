# clipboard

The last things you copied, and a notes file.

| Chord | What |
|---|---|
| SUPER+ALT+V | a menu of the last ten clips; pick one and it is pasted |
| SUPER+ALT+N | appends the clipboard to `~/notes/clips.md` under a timestamp |

Only text is kept; an image in the clipboard reads as empty. The history
lives in `om.store`, so it survives reloads and restarts.

## Install

```sh
om plugin add clipboard
```

Options go in `~/.config/omaestro/rules.d/clipboard.lua`:

```lua
om.use("clipboard").setup({ keep = 25, file = os.getenv("HOME") .. "/clips.md" })
```

## Options

- `history`: the chord for the clip menu (`false`: no history is kept at all).
- `keep`: how many clips to keep (default 10).
- `notes`: the chord that appends the clipboard to the notes file (`false`: off).
- `file`: the notes file (default `~/notes/clips.md`); its directory is created.
