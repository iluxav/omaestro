-- SUPER+ALT+U replaces the selected text with its upper-case form. No
-- model, no shell: the selection comes in, the paste goes out.

om.hotkey("SUPER + ALT + U", function()
  local text = om.selection()
  if text == "" then
    return
  end
  om.paste(text:upper())
end)
