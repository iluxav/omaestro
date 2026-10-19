-- window-island: dock a window on the left, across workspaces, while the
-- other tiles leave room beside it. No edits to the Hyprland config.
--
--   om.use("window-island").setup({ width = 800 })
--
-- SUPER+CTRL+I docks/undocks; SUPER+CTRL+[ / ] narrows/widens.
-- width, step, min_width, min_remaining and gap are logical pixels.

local M = {}
local STORE = "window-island.state"
local source = require("window-island.space")

local function option(value, default)
  if value == nil then return default end
  return value
end

local function integer(value, default, minimum)
  value = tonumber(option(value, default))
  assert(value and value == value and value < math.huge and value >= minimum,
    "window-island: expected a finite number >= " .. minimum)
  return math.floor(value)
end

-- Serialize data as Lua, then quote the entire program for the shell.
-- Neither a monitor name nor a config path is interpreted as shell code.
local function literal(value)
  if type(value) == "string" then return string.format("%q", value) end
  if type(value) ~= "table" then return tostring(value) end
  local fields = {}
  for key, item in pairs(value) do
    fields[#fields + 1] = "[" .. literal(key) .. "]=" .. literal(item)
  end
  table.sort(fields)
  return "{" .. table.concat(fields, ",") .. "}"
end

local function send(request)
  local code = "local request = " .. literal(request) .. "\n" .. source
  local quoted = "'" .. code:gsub("'", "'\"'\"'") .. "'"
  local result = om.shell("hyprctl eval " .. quoted, { timeout = 5 })
  -- Some hyprctl versions exit successfully even when eval failed.
  assert(result:match("^%s*ok%s*$"), "window-island: " .. result)
end

function M.setup(opts)
  opts = opts or {}
  local width = integer(opts.width, 800, 1)
  local step = integer(opts.step, 80, 1)
  local minimum = integer(opts.min_width, 240, 1)
  local remaining = integer(opts.min_remaining, 480, 1)
  local gap = integer(opts.gap, 10, 0)
  local signature = os.getenv("HYPRLAND_INSTANCE_SIGNATURE") or ""
  local state = om.store.get(STORE, {})
  -- Addresses only mean anything inside the compositor that created them.
  if state.instance ~= signature then state = { instance = signature, islands = {} } end
  state.islands = state.islands or {}
  local busy = false

  local function save() om.store.set(STORE, state) end

  local function request(action, name, entry)
    send({ action = action, monitor = name, entry = entry, gap = gap,
      min_width = minimum, min_remaining = remaining })
  end

  local function live(entry)
    for _, win in ipairs(om.windows()) do
      if win.address == entry.address and win.pid == entry.pid and win.class == entry.class then
        return win
      end
    end
  end

  local function release(name)
    local entry = state.islands[name]
    if not entry then return end
    request("release", name, entry)
    state.islands[name] = nil
    save()
  end

  local function sync()
    local gone = {}
    for name, entry in pairs(state.islands) do
      local win = live(entry)
      local monitor = om.monitor(name)
      if not win or not monitor or win.monitor ~= monitor.id then
        gone[#gone + 1] = name
      else
        request("reserve", name, entry)
      end
    end
    for _, name in ipairs(gone) do release(name) end
  end

  -- Different triggers can be in flight together while an IPC call yields.
  local function guarded(fn)
    return function(...)
      if busy then return end
      busy = true
      local ok, err = pcall(fn, ...)
      busy = false
      if not ok then error(err, 0) end
    end
  end

  local function toggle()
    local win = om.window()
    if not win then return end
    for name, entry in pairs(state.islands) do
      if entry.address == win.address then release(name); return end
    end
    if win.fullscreen_mode ~= "none" or (win.workspace_id or 0) < 0 then
      om.notify("Window island", "Leave fullscreen or the special workspace before docking this window.")
      return
    end
    local monitor
    for _, candidate in ipairs(om.monitors()) do
      if candidate.id == win.monitor then monitor = candidate; break end
    end
    if not monitor then return end
    local name = monitor.name
    -- One window per monitor. Replacing it restores the previous window first.
    release(name)
    local entry = {
      address = win.address, pid = win.pid, class = win.class, width = width,
      floating = win.floating, pinned = win.pinned,
      x = win.x, y = win.y, w = win.width, h = win.height,
    }
    state.islands[name] = entry
    save()
    local ok, err = pcall(request, "reserve", name, entry)
    if not ok then
      -- Keep the saved entry if rollback fails, so another toggle can retry.
      pcall(release, name)
      error(err, 0)
    end
  end

  local function adjust(delta)
    local monitor = om.monitor()
    local entry = monitor and state.islands[monitor.name]
    if not entry then return end
    local available = monitor.width - remaining
    if available < minimum then return end
    entry.width = math.max(minimum, math.min(available, entry.width + delta))
    save()
    request("reserve", monitor.name, entry)
  end

  local chord = option(opts.toggle, "SUPER + CTRL + I")
  if chord then om.hotkey(chord, guarded(toggle), { label = "Dock or undock the window island" }) end
  chord = option(opts.narrower, "SUPER + CTRL + bracketleft")
  if chord then om.hotkey(chord, guarded(function() adjust(-step) end), { label = "Narrow the window island" }) end
  chord = option(opts.wider, "SUPER + CTRL + bracketright")
  if chord then om.hotkey(chord, guarded(function() adjust(step) end), { label = "Widen the window island" }) end

  om.on_close({}, guarded(function(win)
    for name, entry in pairs(state.islands) do
      if entry.address == win.address then release(name); return end
    end
  end), { label = "Release the island when its window closes" })
  om.on_workspace(guarded(sync), { label = "Keep the window island across workspaces" })
  om.on_monitor(guarded(sync), { label = "Keep the window island on its monitor" })
  -- Also reapplies after a Hyprland config reload. The compositor releases
  -- our temporary rule if these heartbeats stop (removal, disable or exit).
  om.every("2s", guarded(sync), { label = "Maintain the window island and its cleanup lease" })
  om.after("1s", guarded(sync), { label = "Restore the window island after a rules reload" })
  return M
end

return M
