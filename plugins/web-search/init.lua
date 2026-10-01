-- web-search: SUPER+ALT+I asks for a query and opens it in the browser.
-- The menu is omarchy-menu-input on Omarchy (or walker, wofi, fuzzel, rofi,
-- or `prompt_command` in omaestro.toml); cancelling or an empty line does
-- nothing.
--
--   om.use("web-search").setup({})
--
-- Options:
--   chord = "SUPER + ALT + I"
--   url   = "https://duckduckgo.com/?q="   the query is percent-encoded and appended
--   open  = "xdg-open"                     the command that takes the URL

local M = {}

-- Percent-encodes everything but unreserved characters.
local function encode(text)
  return (text:gsub("[^%w%-%._~]", function(c)
    return string.format("%%%02X", c:byte())
  end))
end

function M.setup(opts)
  opts = opts or {}
  local base = opts.url or "https://duckduckgo.com/?q="
  local open = opts.open or "xdg-open"
  om.hotkey(opts.chord or "SUPER + ALT + I", function()
    local query = om.prompt("Search the web")
    if not query then
      return
    end
    om.spawn(open .. " '" .. base .. encode(query) .. "'")
  end)
  return M
end

return M
