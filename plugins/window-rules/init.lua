-- window-rules: what to do with a window when it appears, decided in Lua:
-- float it, center it, send it to a workspace, or anything else. For a rule
-- that always applies, a Hyprland window rule is the better tool; this is
-- for when the decision needs logic, or you want it next to your other
-- rules. Also: a note when a monitor comes or goes, and a focus log for
-- finding an app's class.
--
--   om.use("window-rules").setup({})
--
-- Options:
--   rules = {
--     { class = "^org%.gnome%.Calculator$", float = true, center = true },
--     -- { class = "^[Ss]potify$", workspace = 9 },
--   }
--     each rule: class and/or title (Lua patterns), then float = true,
--     center = true, workspace = <number>, and/or act = function(win).
--     Applied once per window: when it opens, or for windows that were
--     already there when the rules loaded, the first time it gets focus.
--   notify_monitor = true    notify when a monitor is added or removed
--   log_focus      = false   log every focus change to the journal
--                            (journalctl --user -u omaestro -f shows classes)

local M = {}

local function option(value, default)
  if value == nil then
    return default
  end
  return value
end

function M.setup(opts)
  opts = opts or {}

  local rules = opts.rules or {
    { class = "^org%.gnome%.Calculator$", float = true, center = true },
  }
  for _, rule in ipairs(rules) do
    local seen = {}
    local function apply(win)
      if seen[win.address] then
        return
      end
      seen[win.address] = true
      if rule.workspace then
        win:to_workspace(rule.workspace)
      end
      if rule.float then
        win:float(true)
      end
      if rule.center then
        win:center()
      end
      if rule.act then
        rule.act(win)
      end
    end
    local matcher = { class = rule.class, title = rule.title }
    local name = rule.label or ("Window rule for " .. (rule.class or rule.title or "every window"))
    om.on_open(matcher, apply, { label = name .. " (on open)" })
    om.on_focus(matcher, apply, { label = name .. " (on focus)" })
  end

  if option(opts.notify_monitor, true) then
    -- {name, change = "added" | "removed" | "focused"}
    om.on_monitor(function(mon)
      if mon.change ~= "focused" then
        om.notify("Monitor " .. mon.change, mon.name)
      end
    end, { label = "Notify on monitor changes" })
  end

  if opts.log_focus then
    om.on_focus({}, function(win)
      om.log("focus", win.class, win.title)
    end, { label = "Log focus changes" })
  end
  return M
end

return M
