# downloads

A notification naming the file when something lands in `~/Downloads`.

## Install

```sh
om plugin add downloads
```

Another directory, in `~/.config/omaestro/rules.d/downloads.lua`:

```lua
om.use("downloads").setup({ dir = os.getenv("HOME") .. "/Inbox" })
```

## Options

- `dir`: the directory to watch (default `~/Downloads`). A directory that
  does not exist is logged and skipped.
