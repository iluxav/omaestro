-- SUPER+ALT+D types today's date where the cursor is: 2026-09-30.
--
-- `om.type` types through a virtual keyboard, which suits short ASCII
-- snippets. For longer text, or text some app drops, `om.paste` is the
-- more reliable route.

om.hotkey("SUPER + ALT + D", function()
  om.type(os.date("%Y-%m-%d"))
end)
