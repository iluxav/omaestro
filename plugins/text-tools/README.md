# text-tools

Snippets and selection transforms, no model involved.

| Chord | What |
|---|---|
| SUPER+ALT+D | types today's date where the cursor is: `2026-09-30` |
| SUPER+ALT+U | replaces the selected text with its upper-case form |

## Install

```sh
om plugin add text-tools
```

It installs with its defaults and lists them; `om plugin configure text-tools`
opens them as a form in your editor. Either way they are written to
`~/.config/omaestro/rules.d/text-tools.lua`, which you can also edit.

Your own snippets go in `~/.config/omaestro/rules.d/text-tools.lua`:

```lua
om.use("text-tools").setup({
  snippets = {
    ["SUPER + ALT + D"] = function() return os.date("%Y-%m-%d") end,
    ["SUPER + ALT + A"] = "me@example.com",
  },
})
```

## Options

- `snippets`: chord → text, or a function returning it. Typed with
  `om.type`, which is right for short ASCII; for longer text return it
  from a function and call `om.paste` yourself.
- `upper`: the chord that upper-cases the selection (`false`: off).
- `date`: the chord that types today's date (default `SUPER + ALT + D`;
  `false`: off). Used when no `snippets` are given.
- `date_format`: how the date is written, an `os.date` format (default
  `%Y-%m-%d`; `%d.%m.%Y` gives `30.09.2026`).
