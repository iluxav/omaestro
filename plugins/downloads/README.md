# downloads

A notification naming the file when something lands in `~/Downloads`.

## Install

```sh
om plugin add downloads
```

It asks for its options as it installs (Enter keeps a default);
`om plugin configure downloads` changes them later. Both write
`~/.config/omaestro/rules.d/downloads.lua`, which you can also edit.

Another directory, in `~/.config/omaestro/rules.d/downloads.lua`:

```lua
om.use("downloads").setup({ dir = os.getenv("HOME") .. "/Inbox" })
```

## Options

- `dir`: the directory to watch (default `~/Downloads`). A directory that
  does not exist is logged and skipped.
