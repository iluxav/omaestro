-- SUPER+ALT+T replaces the selected text with its English translation
-- (the model detects the source language). Change `target` for another
-- language.

local target = "English"

om.hotkey("SUPER + ALT + T", function()
  local text = om.selection()
  if text:trim() == "" then
    om.notify("omaestro", "Select some text first")
    return
  end
  local translated = om.llm(text, {
    system = "Translate the user's text to " .. target .. ". Keep the formatting. "
      .. "Reply with the translation only, no quotes, no comments.",
  })
  om.paste(translated:trim())
end)
