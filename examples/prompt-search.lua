-- SUPER+ALT+S asks for a search and opens it in the browser. om.prompt
-- returns nil when the menu is cancelled or left empty.
--
-- The menu is omarchy-menu-input on Omarchy, or walker / wofi / fuzzel /
-- rofi, or whatever `prompt_command` in omaestro.toml says.

om.hotkey("SUPER + ALT + S", function()
  local query = om.prompt("Search the web")
  if not query then
    return
  end
  -- Percent-encode everything but unreserved characters.
  local encoded = query:gsub("[^%w%-%._~]", function(c)
    return string.format("%%%02X", c:byte())
  end)
  om.shell("xdg-open 'https://duckduckgo.com/?q=" .. encoded .. "'")
end)
