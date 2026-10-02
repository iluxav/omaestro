-- window-halves: put the focused window on a half, a third or a corner of
-- its monitor with one chord, Rectangle-style. CTRL+ALT+Left/Right put it
-- on the left/right half, CTRL+ALT+Up fills the screen, CTRL+ALT+Down
-- centers it; SUPER+ALT+C floats and centers the window, or tiles it back.
-- The window floats and takes the exact area; the bar's space is left alone.
--
--   om.use("window-halves").setup({})
--
-- Options:
--   chord  = "CTRL + ALT + "   the modifiers in front of each key (Omarchy uses
--                              every SUPER+arrow combination itself)
--   keys   = { Left = "left", Right = "right", Up = "max", Down = "center" }
--            key -> place: left, right, top, bottom, top-left, top-right,
--            bottom-left, bottom-right, left-third, middle-third, right-third,
--            left-two-thirds, right-two-thirds, center, max, or fractions
--            {x = 0, y = 0, w = 0.5, h = 1}
--   toggle = "SUPER + ALT + C"  float-and-center, or tile back (false: off)

local M = {}

-- What each place is called in the panel.
local NAMES = {
  left = "Left half", right = "Right half", top = "Top half", bottom = "Bottom half",
  ["top-left"] = "Top-left corner", ["top-right"] = "Top-right corner",
  ["bottom-left"] = "Bottom-left corner", ["bottom-right"] = "Bottom-right corner",
  ["left-third"] = "Left third", ["middle-third"] = "Middle third", ["right-third"] = "Right third",
  ["left-two-thirds"] = "Left two thirds", ["right-two-thirds"] = "Right two thirds",
  center = "Center", max = "Fill the screen",
}

local function option(value, default)
  if value == nil then
    return default
  end
  return value
end

function M.setup(opts)
  opts = opts or {}

  local prefix = opts.chord or "CTRL + ALT + "
  local keys = opts.keys or { Left = "left", Right = "right", Up = "max", Down = "center" }
  for key, place in pairs(keys) do
    om.hotkey(prefix .. key, function()
      local win = om.window()
      if win then
        win:place(place)
      end
    end, { label = NAMES[place] or "Place the window" })
  end

  local toggle = option(opts.toggle, "SUPER + ALT + C")
  if toggle then
    om.hotkey(toggle, function()
      local win = om.window()
      if not win then
        return
      end
      om.dispatch("hl.dsp.window.float()")
      if not win.floating then
        om.dispatch("hl.dsp.window.center()")
      end
    end, { label = "Float and center, or tile back" })
  end
  return M
end

return M
