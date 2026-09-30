-- Float and center a window the first time it gets focus, which for a new
-- window is when it opens. Here: GNOME Calculator. Find another app's class
-- with `om eval 'return om.window()'` while it is focused, or
-- `hyprctl -j activewindow`.
--
-- For a rule that should always apply, a Hyprland window rule is the better
-- tool. This one is for when the decision needs logic.

local seen = {}

om.on_focus({ class = "^org%.gnome%.Calculator$" }, function(win)
  if seen[win.address] then
    return
  end
  seen[win.address] = true
  om.dispatch("hl.dsp.window.float()")
  om.dispatch("hl.dsp.window.center()")
end)
