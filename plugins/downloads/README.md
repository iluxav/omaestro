# downloads

A notification naming the file when something lands in `~/Downloads`.

## Install

```sh
om plugin add https://github.com/iluxav/omaestro/tree/main/plugins/downloads
```

It installs with its defaults and lists them; `--set key=value` after
the URL (once per option) installs it with yours instead, and
`om plugin configure downloads` opens them as a form in your editor.
Either way they are written to `~/.config/omaestro/rules.d/downloads.lua`,
which you can also edit.

Another directory, in `~/.config/omaestro/rules.d/downloads.lua`:

```lua
om.use("downloads").setup({ dir = os.getenv("HOME") .. "/Inbox" })
```

## Options

- `dir`: the directory to watch (default `~/Downloads`). A directory that
  does not exist is logged and skipped.
