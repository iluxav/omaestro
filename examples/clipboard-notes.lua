-- SUPER+ALT+N appends the clipboard to a notes file with a timestamp, and
-- says so. Plain Lua does the file; om.clipboard reads the text (an image
-- in the clipboard reads as "").

local notes = os.getenv("HOME") .. "/notes/clips.md"

om.hotkey("SUPER + ALT + N", function()
  local text = om.clipboard():trim()
  if text == "" then
    om.notify("Clip", "Nothing (or no text) in the clipboard")
    return
  end
  om.shell("mkdir -p " .. os.getenv("HOME") .. "/notes")
  local file, err = io.open(notes, "a")
  if not file then
    error("cannot open " .. notes .. ": " .. tostring(err))
  end
  file:write("\n## ", os.date("%Y-%m-%d %H:%M"), "\n\n", text, "\n")
  file:close()
  om.notify("Clip", "Saved " .. #text .. " characters to notes/clips.md")
end)
