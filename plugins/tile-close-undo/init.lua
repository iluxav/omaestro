-- tile-close-undo: SUPER+W closes the focused window a few seconds late, and
-- SUPER+Z in the meantime brings it back as it was. Until then the window
-- only waits, hidden on a special workspace: the app keeps running, so its
-- content, scroll position and unsaved text are all there when it returns.
--
--   om.use("tile-close-undo").setup({})
--
-- SUPER+W is Omarchy's own "Close window", and omaestro does not take a
-- chord that is already bound: add hl.unbind("SUPER + W") to
-- ~/.config/hypr/bindings.lua, or turn override on (om override on).
--
-- Options:
--   chord  = "SUPER + W"   close the focused window, with undo
--   undo   = "SUPER + Z"   bring back the window closed last
--   delay  = 3             seconds before the window really closes
--   notify = false         a notification on each close, naming the undo chord

local M = {}

-- Where closed windows wait. No bind shows it.
local TRASH = "special:om-tile-close-undo"
-- The workspace each waiting window came from, by address, kept in
-- om.store: a reload or a restart drops the timers, and setup puts the
-- windows they left waiting back.
local STORE = "tile-close-undo.waiting"

-- Where to send a window back: its workspace id, or a special or named
-- workspace by name.
local function home(win)
  if win.workspace_id and win.workspace_id > 0 then
    return win.workspace_id
  end
  if win.workspace:sub(1, 8) == "special:" then
    return win.workspace
  end
  return "name:" .. win.workspace
end

local function remember(address, workspace)
  local waiting = om.store.get(STORE, {})
  waiting[address] = workspace
  om.store.set(STORE, waiting)
end

local function forget(address)
  local waiting = om.store.get(STORE, {})
  if waiting[address] ~= nil then
    waiting[address] = nil
    om.store.set(STORE, waiting)
  end
end

local function bring_back(win, workspace, follow)
  forget(win.address)
  win:to_workspace(workspace, follow)
  if follow then
    win:focus()
  end
end

function M.setup(opts)
  opts = opts or {}

  local delay = math.max(1, math.floor(tonumber(opts.delay) or 3))
  local undo = opts.undo or "SUPER + Z"
  -- {win, workspace, timer} for each waiting window, the newest last.
  local waiting = {}

  local function drop(entry)
    for i, e in ipairs(waiting) do
      if e == entry then
        table.remove(waiting, i)
        return
      end
    end
  end

  -- Windows left waiting by the rules before these: back where they came
  -- from, without switching workspace.
  local left = om.store.get(STORE, {})
  for _, win in ipairs(om.windows({ workspace = TRASH })) do
    local here = om.workspace()
    bring_back(win, left[win.address] or (here and here.id) or 1, false)
  end
  if next(left) ~= nil then
    om.store.set(STORE, {})
  end

  om.hotkey(opts.chord or "SUPER + W", function()
    local win = om.window()
    if not win or win.workspace == TRASH then
      return
    end
    local entry = { win = win, workspace = home(win) }
    remember(win.address, entry.workspace)
    win:to_workspace(TRASH, false)
    table.insert(waiting, entry)
    entry.timer = om.after(delay .. "s", function()
      drop(entry)
      forget(win.address)
      local live = win:refresh()
      if not live then
        return
      end
      live:close()
      -- An app that asks first ("Save changes?") is still open, hidden.
      om.after("5s", function()
        local still = win:refresh()
        if still and still.workspace == TRASH then
          bring_back(still, entry.workspace, true)
          om.notify(win.class .. " did not close", "It may be asking something; it is back where it was.")
        end
      end, { label = "Check that " .. win.class .. " closed" })
    end, { label = "Close " .. win.class })
    if opts.notify then
      om.notify("Closed " .. win.class, undo .. " within " .. delay .. " s brings it back")
    end
  end, { label = "Close the window, with undo" })

  om.hotkey(undo, function()
    while #waiting > 0 do
      local entry = table.remove(waiting)
      entry.timer:cancel()
      local live = entry.win:refresh()
      if live then
        bring_back(live, entry.workspace, true)
        return
      end
      forget(entry.win.address)
    end
    om.notify("Nothing to bring back", "A closed window can be brought back for " .. delay .. " s")
  end, { label = "Bring back the window closed last" })

  return M
end

return M
