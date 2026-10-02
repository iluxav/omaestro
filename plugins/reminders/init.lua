-- reminders: SUPER+ALT+R asks how many minutes and reminds you then; a
-- stretch reminder every 45 minutes; a note every day at 17:30.
--
--   om.use("reminders").setup({})
--
-- Options (false switches one off):
--   ask     = "SUPER + ALT + R"
--   stretch = "45m"        an interval: "30s", "5m", "1h", "1h30m"
--   daily   = { ["17:30"] = "Half an hour left. What is unfinished?" }   time -> text

local M = {}

local function option(value, default)
  if value == nil then
    return default
  end
  return value
end

function M.setup(opts)
  opts = opts or {}

  local ask = option(opts.ask, "SUPER + ALT + R")
  if ask then
    om.hotkey(ask, function()
      local minutes = math.floor(tonumber(om.prompt("Remind me in minutes")) or 0)
      if minutes < 1 then
        return
      end
      -- The count survives restarts in om.store.
      local n = om.store.get("reminders", 0) + 1
      om.store.set("reminders", n)
      om.after(minutes .. "m", function()
        om.notify("Reminder " .. n, minutes .. " minutes are up")
      end, { label = "Reminder " .. n })
    end, { label = "Remind me in N minutes" })
  end

  local stretch = option(opts.stretch, "45m")
  if stretch then
    om.every(stretch, function()
      om.notify("Stretch", stretch .. " at the desk. Stand up for a minute.")
    end, { label = "Stand-up reminder every " .. stretch })
  end

  local daily = option(opts.daily, { ["17:30"] = "Half an hour left. What is unfinished?" })
  for time, text in pairs(daily or {}) do
    om.at(time, function()
      om.notify(time, text)
    end, { label = "Daily note at " .. time })
  end
  return M
end

return M
