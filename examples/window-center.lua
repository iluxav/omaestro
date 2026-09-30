-- SUPER+ALT+C floats the focused window and centers it; pressing it again
-- tiles the window back. om.window() says whether it is floating right
-- now; om.dispatch runs Hyprland dispatchers.

om.hotkey("SUPER + ALT + C", function()
  local win = om.window()
  if not win then
    return
  end
  om.dispatch("hl.dsp.window.float()")
  if not win.floating then
    om.dispatch("hl.dsp.window.center()")
  end
end)
