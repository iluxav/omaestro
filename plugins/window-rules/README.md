# window-rules

What to do with a window when it appears, decided in Lua: float it, center
it, send it to a workspace, or run any function on it. By default GNOME
Calculator opens floating and centered, and a monitor being added or
removed is announced.

For a rule that always applies, a Hyprland window rule is the better tool.
This is for when the decision needs logic, or when you want it next to your
other rules.

## Install

```sh
om plugin add window-rules
```

It installs with its defaults and lists them; `om plugin configure window-rules`
opens them as a form in your editor. Either way they are written to
`~/.config/omaestro/rules.d/window-rules.lua`, which you can also edit.

Rules go in `~/.config/omaestro/rules.d/window-rules.lua`:

```lua
om.use("window-rules").setup({
  rules = {
    { class = "^org%.gnome%.Calculator$", float = true, center = true },
    { class = "^[Ss]potify$", workspace = 9 },
    { title = "Picture%-in%-Picture", act = function(win) win:pin(true) end },
  },
  log_focus = true,
})
```

## Options

- `rules`: a list. Each has `class` and/or `title` (Lua patterns), then
  `float = true`, `center = true`, `workspace = <number>`, and/or
  `act = function(win)`. A rule applies once per window: when it opens,
  or, for windows that were already there when the rules loaded, the first
  time it gets focus.
- `notify_monitor`: notify when a monitor is added or removed (default `true`).
- `log_focus`: log every focus change with class and title (default
  `false`); `journalctl --user -u omaestro -f` then shows the classes to
  match on. `om eval 'return om.window().class'` does the same for the
  focused window.
