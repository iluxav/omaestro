# reminders

| What | When |
|---|---|
| SUPER+ALT+R asks "Remind me in minutes" and notifies you then | on the chord |
| "Stand up for a minute" | every 45 minutes |
| "Half an hour left. What is unfinished?" | every day at 17:30 |

The prompt is Omarchy's menu (or walker, wofi, fuzzel, rofi, or
`prompt_command` in `omaestro.toml`).

## Install

```sh
om plugin add reminders
```

Options go in `~/.config/omaestro/rules.d/reminders.lua`:

```lua
om.use("reminders").setup({
  stretch = "1h",
  daily = { ["09:00"] = "Plan the day", ["17:30"] = "Wrap up" },
})
```

## Options

- `ask`: the chord (`false`: off).
- `stretch`: the interval (`"30s"`, `"5m"`, `"1h"`, `"1h30m"`; `false`: off).
- `daily`: a table of `["HH:MM"] = "text"` (`{}` for none).
