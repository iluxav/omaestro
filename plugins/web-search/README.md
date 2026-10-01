# web-search

SUPER+ALT+I asks for a query and opens it in the browser.

The prompt is Omarchy's menu (or walker, wofi, fuzzel, rofi, or
`prompt_command` in `omaestro.toml`); cancelling or an empty line does
nothing. The query is percent-encoded and appended to the search URL.

## Install

```sh
om plugin add web-search
```

Another engine, in `~/.config/omaestro/rules.d/web-search.lua`:

```lua
om.use("web-search").setup({ url = "https://www.google.com/search?q=" })
```

## Options

- `chord`: the hotkey (default `SUPER + ALT + I`; SUPER+ALT+S is Omarchy's scratchpad).
- `url`: the search URL the query is appended to (default DuckDuckGo).
- `open`: the command that opens the URL (default `xdg-open`).
