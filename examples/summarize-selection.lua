-- SUPER+ALT+M summarizes the selected text with the local model and shows
-- the summary as a notification, leaving the text alone. Handy for a long
-- page or mail: select, press, read.

om.hotkey("SUPER + ALT + M", function()
  local text = om.selection()
  if text:trim() == "" then
    om.notify("omaestro", "Select some text first")
    return
  end
  local summary = om.llm(text, {
    system = "Summarize the user's text in at most three short sentences. "
      .. "Reply with the summary only.",
  })
  om.notify("Summary", summary:trim())
end)
