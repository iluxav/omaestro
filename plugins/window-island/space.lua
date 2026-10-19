-- Evaluated by hyprctl inside Hyprland, not in omaestro's Lua state.
-- A temporary workspace gap reserves space for normal tiles without
-- changing display modes, scale, HDR, bars or monitor configuration.
return [=[
local key = "__omaestro_window_island_v1"
local manager = rawget(_G, key)
if not manager then
  manager = { slots = {} }
  rawset(_G, key, manager)
end

local function window(entry)
  local win = hl.get_window("address:" .. entry.address)
  if win and win.mapped and win.pid == entry.pid and win.class == entry.class then return win end
end

local function dispatch(action, args, entry)
  args.window = "address:" .. entry.address
  hl.dispatch(hl.dsp.window[action](args))
end

local function restore(entry)
  if not window(entry) then return end
  dispatch("pin", { action = "disable" }, entry)
  dispatch("float", { action = entry.floating and "enable" or "disable" }, entry)
  if entry.floating then
    dispatch("resize", { x = entry.w, y = entry.h }, entry)
    dispatch("move", { x = entry.x, y = entry.y }, entry)
    if entry.pinned then dispatch("pin", { action = "enable" }, entry) end
  end
end

local function release(name, entry)
  local slot = manager.slots[name]
  if slot then
    slot.rule:set_enabled(false)
    manager.slots[name] = nil
  end
  if entry then restore(entry) end
end

if request.action == "release" then
  release(request.monitor, request.entry)
  return
end

local entry = request.entry
local win = window(entry)
local monitor = hl.get_monitor(request.monitor)
if not win or not monitor or not win.monitor or win.monitor.name ~= monitor.name then
  release(request.monitor, entry)
  return
end

local physical_w, physical_h = monitor.width, monitor.height
if monitor.transform % 2 == 1 then physical_w, physical_h = physical_h, physical_w end
local reserved = monitor.reserved
local usable_w = math.floor(physical_w / monitor.scale) - reserved.left - reserved.right
local usable_h = math.floor(physical_h / monitor.scale) - reserved.top - reserved.bottom
local gaps = hl.get_config("general.gaps_out")
if type(gaps) == "number" then gaps = { top = gaps, right = gaps, bottom = gaps, left = gaps } end
local width = math.min(entry.width, usable_w - request.min_remaining - gaps.left - gaps.right)
local border = hl.get_config("general.border_size") or 0
local inset = request.gap + border
if width < request.min_width or width <= 2 * inset or usable_h <= 2 * inset then
  release(request.monitor, entry)
  error("window-island: monitor is too small for the island and the remaining tiles")
end
width = math.floor(width)

local slot = manager.slots[request.monitor]
local selector = "m[" .. request.monitor .. "]"
local config = table.concat({ width, gaps.top, gaps.right, gaps.bottom, gaps.left }, ":")
if not slot or slot.config ~= config or slot.selector ~= selector then
  -- Include special workspaces so scratchpad tiles overlay the remaining
  -- area to the right. Replacing our selector keeps heartbeats idempotent.
  local rule = hl.workspace_rule({
    workspace = selector,
    gaps_out = { top = gaps.top, right = gaps.right, bottom = gaps.bottom, left = gaps.left + width },
  })
  -- Retire the old normal-only selector when upgrading a running island.
  if slot and slot.selector ~= selector then slot.rule:set_enabled(false) end
  slot = { rule = rule, config = config, selector = selector }
  manager.slots[request.monitor] = slot
end
slot.entry = entry
slot.beats = 5

local x = math.floor(monitor.x + reserved.left + inset)
local y = math.floor(monitor.y + reserved.top + inset)
local w, h = math.floor(width - 2 * inset), math.floor(usable_h - 2 * inset)
if not win.floating then dispatch("float", { action = "enable" }, entry) end
if not win.pinned then dispatch("pin", { action = "enable" }, entry) end
-- Goal geometry, so animations don't cause repeated resize dispatches.
local at, size = win.at, win.size
if size.x ~= w or size.y ~= h then dispatch("resize", { x = w, y = h }, entry) end
if at.x ~= x or at.y ~= y then dispatch("move", { x = x, y = y }, entry) end

if not manager.timer then
  manager.timer = hl.timer(function()
    local expired = {}
    for name, current in pairs(manager.slots) do
      current.beats = current.beats - 1
      if current.beats <= 0 or not window(current.entry) then expired[#expired + 1] = name end
    end
    for _, name in ipairs(expired) do release(name, manager.slots[name].entry) end
    if not next(manager.slots) then
      manager.timer:set_enabled(false)
      manager.timer = nil
    end
  end, { timeout = 2000, type = "repeat" })
end
]=]
