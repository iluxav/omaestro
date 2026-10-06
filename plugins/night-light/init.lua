-- night-light: make the screen warmer at night and normal again in the
-- morning, through hyprsunset (Omarchy ships it): warm at 20:00, normal at
-- 07:00, a hotkey to switch now, and true colors while an image editor has
-- focus. hyprsunset is started when it is not running.
--
--   om.use("night-light").setup({})
--
-- Options (false switches one off):
--   warm_at     = "20:00"              when the screen turns warm
--   normal_at   = "07:00"              when it turns normal again
--   temperature = 4000                 kelvin at night; 6000 is hyprsunset's day
--   toggle      = "SUPER + ALT + S"    warm or normal now, until the next switch
--   true_colors = { "^[Gg]imp$", "^org%.inkscape%.Inkscape$", "^darktable$" }
--                 class patterns: normal colors while one of them has focus
--   notify      = false                a notification on each scheduled switch

local M = {}

local function option(value, default)
  if value == nil then
    return default
  end
  return value
end

-- "HH:MM" strings compare as times do.
local function at_night(now, warm_at, normal_at)
  if warm_at > normal_at then -- the usual: the night spans midnight
    return now >= warm_at or now < normal_at
  end
  return now >= warm_at and now < normal_at
end

function M.setup(opts)
  opts = opts or {}
  local warm_at = option(opts.warm_at, "20:00")
  local normal_at = option(opts.normal_at, "07:00")
  local temperature = option(opts.temperature, 4000)
  local notify = option(opts.notify, false)
  local warm = false -- what the screen is set to
  local editing = false -- an image editor has focus: normal colors meanwhile

  -- hyprsunset answers on its socket only while it runs; Omarchy starts it
  -- at login, and this starts it when it is not there.
  local function set(args)
    local ok, err = pcall(om.shell, "pgrep -x hyprsunset >/dev/null || { setsid -f hyprsunset >/dev/null 2>&1; sleep 0.5; }; hyprctl hyprsunset " .. args)
    if not ok then
      om.log("night-light", "hyprsunset: " .. tostring(err))
    end
  end

  local function apply(say)
    if warm and not editing then
      set("temperature " .. temperature)
    else
      set("identity")
    end
    if say then
      om.notify("Night light", warm and ("warm, " .. temperature .. " K") or "normal colors")
    end
  end

  local function scheduled()
    if warm_at and normal_at then
      warm = at_night(os.date("%H:%M"), warm_at, normal_at)
    end
    apply(false)
  end

  -- On load, and after a sleep that may have crossed a switch time.
  scheduled()
  om.on_wake(scheduled, { label = "Night light: set the screen after a wake" })

  if warm_at then
    om.at(warm_at, function()
      warm = true
      apply(notify)
    end, { label = "Make the screen warmer at " .. warm_at })
  end
  if normal_at then
    om.at(normal_at, function()
      warm = false
      apply(notify)
    end, { label = "Normal colors again at " .. normal_at })
  end

  local toggle = option(opts.toggle, "SUPER + ALT + S")
  if toggle then
    om.hotkey(toggle, function()
      warm = not warm
      apply(true)
    end, { label = "Night light on or off, until the next switch" })
  end

  local editors = option(opts.true_colors, { "^[Gg]imp$", "^org%.inkscape%.Inkscape$", "^darktable$" })
  if editors and #editors > 0 then
    om.on_focus({}, function(win)
      local now = false
      for _, pattern in ipairs(editors) do
        if win.class:match(pattern) then
          now = true
        end
      end
      if now ~= editing then
        editing = now
        if warm then
          apply(false)
        end
      end
    end, { label = "True colors while an image editor has focus" })
  end
  return M
end

return M
