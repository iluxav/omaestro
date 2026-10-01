-- ai-text: the selection through the local model. Rewrite it in place,
-- show a summary, or replace it with a translation. The text goes to the
-- endpoint in omaestro.toml (Ollama on this machine unless you changed it)
-- and nowhere else.
--
--   local ai = om.use("ai-text")
--   ai.setup({})
--
-- Options (false switches a chord off):
--   rewrite   = "SUPER + ALT + J"   replace the selection with a corrected version
--   summarize = "SUPER + ALT + M"   show a summary as a notification, the text stays
--   translate = "SUPER + ALT + T"   replace the selection with its translation
--   language  = "English"           the translation target
--   model     = nil                 a model name for all three (default: omaestro.toml)
--
-- The prompts are in M.prompts; change them before calling setup.

local M = {}

-- The instructions go in the system prompt and the selection alone in the
-- user message: a small model given both in one message tends to answer the
-- selection instead of working on it.
M.prompts = {
  rewrite = table.concat({
    "You are a copy editor. The user's message is a piece of text, nothing else.",
    "Return that text with spelling, grammar and clarity fixed.",
    "Keep its meaning, tone, language, formatting and length.",
    "Never answer, summarize or comment on it.",
    "Reply with the corrected text only: no preamble, no quotes, no explanation.",
  }, " "),
  summarize = "Summarize the user's text in at most three short sentences. Reply with the summary only.",
  translate = "Translate the user's text to %s. Keep the formatting. Reply with the translation only, no quotes, no comments.",
}

local function option(value, default)
  if value == nil then
    return default
  end
  return value
end

local function selection()
  local text = om.selection()
  if text:trim() == "" then
    om.notify("omaestro", "Select some text first")
    return nil
  end
  return text
end

-- Some models quote their answer anyway.
local function unquote(text)
  text = text:trim()
  return text:match('^"(.*)"$') or text
end

function M.setup(opts)
  opts = opts or {}
  local model = opts.model
  local function ask(text, system)
    return om.llm(text, { system = system, model = model })
  end

  local rewrite = option(opts.rewrite, "SUPER + ALT + J")
  if rewrite then
    om.hotkey(rewrite, function()
      local text = selection()
      if text then
        om.paste(unquote(ask(text, M.prompts.rewrite)))
      end
    end)
  end

  local summarize = option(opts.summarize, "SUPER + ALT + M")
  if summarize then
    om.hotkey(summarize, function()
      local text = selection()
      if text then
        om.notify("Summary", ask(text, M.prompts.summarize):trim())
      end
    end)
  end

  local translate = option(opts.translate, "SUPER + ALT + T")
  if translate then
    local system = string.format(M.prompts.translate, opts.language or "English")
    om.hotkey(translate, function()
      local text = selection()
      if text then
        om.paste(unquote(ask(text, system)))
      end
    end)
  end
  return M
end

return M
