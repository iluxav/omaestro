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
