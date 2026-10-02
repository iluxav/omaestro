-- window-mode: SUPER+ALT+W enters a mode where single keys place the
-- focused window until Escape (or q) leaves it: h j k l halves, H L thirds,
-- c center, m max, f float. The hint shows as a notification on entry.
-- Modes are Hyprland submaps, so the keys show in `hyprctl binds` and the
-- keybindings menu like any other bind.
--
--   om.use("window-mode").setup({})
--
-- Options:
--   chord = "SUPER + ALT + W"
--   keys  = { h = "left", j = "bottom", k = "top", l = "right",
--             ["SHIFT + h"] = "left-third", ["SHIFT + l"] = "right-third",
--             c = "center", m = "max", f = "float" }
--           key -> a place name, {x=, y=, w=, h=}, "float", or a function
--   hint  = "Window mode: h j k l halves, H L thirds, c center, m max, f float, Esc"
--   exit  = { "q" }   keys that leave the mode, besides Escape
--   once  = false     leave the mode after one key

local M = {}

local function action(what)
  if type(what) == "function" then
    return what
  end
  return function()
    local win = om.window()
    if not win then
      return
    end
    if what == "float" then
      win:float()
    else
      win:place(what)
    end
  end
end

function M.setup(opts)
  opts = opts or {}
  local keys = opts.keys or {
    h = "left",
    j = "bottom",
    k = "top",
    l = "right",
    ["SHIFT + h"] = "left-third",
    ["SHIFT + l"] = "right-third",
    c = "center",
    m = "max",
    f = "float",
  }
  local bound = {}
  for key, what in pairs(keys) do
    bound[key] = action(what)
  end
  local hint = opts.hint
  if hint == nil then
    hint = "Window mode: h j k l halves, H L thirds, c center, m max, f float, Esc"
  end
  om.mode(opts.chord or "SUPER + ALT + W", bound, {
    label = "Window mode",
    hint = hint or nil,
    exit = opts.exit or { "q" },
    once = opts.once or false,
  })
  return M
end

return M
