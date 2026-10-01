-- apps: bring an app to the front or start it, and arrange a desk with one
-- chord. SUPER+ALT+B is the browser; SUPER+ALT+L lays out browser, editor
-- and terminal.
--
--   om.use("apps").setup({})
--
-- Options:
--   focus = {
--     ["SUPER + ALT + B"] = { class = "^firefox$", command = "uwsm-app -- firefox" },
--   }
--     chord -> the window to focus (class and/or title, Lua patterns) and the
--     command that starts the app when no window matches. om.focus waits up
--     to 15 seconds for the new window. `uwsm-app --` puts the app in its own
--     systemd scope, the way Omarchy launches everything; plain commands work.
--   layouts = {
--     ["SUPER + ALT + L"] = {
--       { class = "^firefox$", place = "left" },
--       { class = "^code$", place = "right" },
--       { class = "^foot$", workspace = 2, place = "max", all = true },
--     },
--   }
--     chord -> entries for om.layout: matchers, an optional workspace, and a
--     place (halves, thirds, corners, center, max). Only the first matching
--     window is placed unless all = true; apps that are not running are skipped.
--   app_hotkeys = {
--     ["^firefox$"] = { ["CTRL + SHIFT + D"] = function() om.notify("firefox", "hello") end },
--   }
--     app (class pattern, or {class=, title=}) -> chord -> handler, bound only
--     while that app has focus (om.app_hotkey): every other app keeps the chord.
--     None by default.

local M = {}

function M.setup(opts)
  opts = opts or {}

  local focus = opts.focus or {
    ["SUPER + ALT + B"] = { class = "^firefox$", command = "uwsm-app -- firefox" },
  }
  for chord, app in pairs(focus) do
    om.hotkey(chord, function()
      local win = om.focus({ class = app.class, title = app.title }, app.command)
      if not win then
        om.notify("apps", (app.command or app.class or "the app") .. " did not show a window in time")
      end
    end)
  end

  local layouts = opts.layouts or {
    ["SUPER + ALT + L"] = {
      { class = "^firefox$", place = "left" },
      { class = "^code$", place = "right" },
      { class = "^foot$", workspace = 2, place = "max", all = true },
    },
  }
  for chord, entries in pairs(layouts) do
    om.hotkey(chord, function()
      local placed = om.layout(entries)
      om.notify("Layout", placed .. " window(s) placed")
    end)
  end

  for app, keys in pairs(opts.app_hotkeys or {}) do
    for chord, handler in pairs(keys) do
      om.app_hotkey(app, chord, handler)
    end
  end
  return M
end

return M
