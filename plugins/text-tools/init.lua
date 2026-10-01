-- text-tools: type a snippet where the cursor is, or transform the
-- selection; no model involved. SUPER+ALT+D types today's date,
-- SUPER+ALT+U replaces the selection with its upper-case form.
--
--   om.use("text-tools").setup({})
--
-- Options:
--   snippets = { ["SUPER + ALT + D"] = function() return os.date("%Y-%m-%d") end }
--     chord -> the text, or a function returning it. Typed with om.type,
--     which suits short ASCII; for longer text om.paste is the surer route.
--   upper = "SUPER + ALT + U"   (false: off)

local M = {}

local function option(value, default)
  if value == nil then
    return default
  end
  return value
end

function M.setup(opts)
  opts = opts or {}

  local snippets = opts.snippets or {
    ["SUPER + ALT + D"] = function()
      return os.date("%Y-%m-%d")
    end,
  }
  for chord, snippet in pairs(snippets) do
    om.hotkey(chord, function()
      local text = snippet
      if type(snippet) == "function" then
        text = snippet()
      end
      if text and text ~= "" then
        om.type(text)
      end
    end)
  end

  local upper = option(opts.upper, "SUPER + ALT + U")
  if upper then
    om.hotkey(upper, function()
      local text = om.selection()
      if text ~= "" then
        om.paste(text:upper())
      end
    end)
  end
  return M
end

return M
