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
--   model     = nil                 a model name for every chord (default: omaestro.toml)
--
-- One chord, a menu of modes: with `modes` set, SUPER+ALT+J asks which
-- mode to apply (the last one used comes first, so Enter repeats it), and
-- the direct rewrite chord is off unless `rewrite` names another chord.
--   modes  = true                   the modes in M.modes, or your own list:
--            { { "More polite", "Rewrite it so it is polite. Keep the meaning." },
--              { "Summarize", "Summarize it in three sentences.", show = true } }
--            label, instruction; show = true notifies instead of replacing
--   menu   = "SUPER + ALT + J"      the chord that opens the menu
--   custom = true                   a last row that asks for a one-off instruction
--
-- The prompts are in M.prompts and the default modes in M.modes; change
-- them before calling setup.

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

-- A mode's instruction goes between these two, so a short one ("make it
-- polite") still reaches a small model as work to do on the text.
M.frame = {
  "The user's message is a piece of text, nothing else. Never answer or comment on it.",
  "Reply with the result only: no preamble, no quotes, no explanation.",
}

M.modes = {
  { "Fix spelling and grammar", "Fix spelling and grammar only. Change nothing else: keep the words, tone, formatting and length." },
  { "Make it clearer", "Rewrite it so it is clear and correct. Keep its meaning, tone, language and formatting." },
  { "Make it more polite", "Rewrite it so it is polite and friendly. Keep its meaning and language." },
  { "Make it shorter", "Make it shorter. Keep its meaning and language." },
  { "Marketing pitch", "Rewrite it as a short, confident marketing pitch. Keep every fact and add none." },
  { "Summarize", "Summarize it in at most three short sentences.", show = true },
}

local CUSTOM = "Custom instruction..."

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

-- { "label", "instruction", show = true } -> { label, instruction, show }
local function check_modes(list)
  if type(list) ~= "table" or #list == 0 then
    error("ai-text: modes must be true or a list of { label, instruction }")
  end
  local modes = {}
  for i, mode in ipairs(list) do
    local label, instruction = mode[1] or mode.label, mode[2] or mode.instruction
    if type(label) ~= "string" or type(instruction) ~= "string" then
      error("ai-text: mode " .. i .. " needs a label and an instruction, both strings")
    end
    modes[#modes + 1] = { label = label, instruction = instruction, show = mode.show == true }
  end
  return modes
end

function M.setup(opts)
  opts = opts or {}
  local model = opts.model
  -- The model can take a while; a notification says it is working until
  -- the answer is back (om.busy goes away when the handler ends).
  local function ask(text, system, doing)
    om.busy(doing or "Rewriting…", text:sub(1, 80))
    return om.llm(text, { system = system, model = model })
  end

  local modes = opts.modes
  if modes == true then
    modes = M.modes
  end
  if modes then
    modes = check_modes(modes)

    -- The instruction wrapped in M.frame; the answer replaces the text in
    -- the window it was selected in, or (show) comes up as a notification.
    local function apply(ctx, instruction, label, show)
      local system = table.concat({ M.frame[1], instruction, M.frame[2] }, " ")
      local answer = ask(ctx.selection, system, (label or "Custom instruction") .. "…")
      if show then
        om.busy()
        om.notify(label, answer:trim())
        return
      end
      -- Again here: a custom instruction's prompt took the focus too.
      if ctx.window then
        ctx.window:focus()
      end
      om.paste(unquote(answer))
    end

    local items = {}
    for _, mode in ipairs(modes) do
      items[#items + 1] = {
        mode.label,
        function(ctx)
          apply(ctx, mode.instruction, mode.label, mode.show)
        end,
      }
    end
    if option(opts.custom, true) then
      items[#items + 1] = {
        CUSTOM,
        function(ctx)
          local instruction = om.prompt("Instruction")
          if instruction and instruction:trim() ~= "" then
            apply(ctx, instruction)
          end
        end,
        remember = false,
      }
    end

    -- om.menu takes the selection and the window before the menu opens,
    -- and puts the last mode used first.
    om.menu(opts.menu or "SUPER + ALT + J", function(ctx)
      if ctx.selection:trim() == "" then
        om.notify("omaestro", "Select some text first")
        return nil
      end
      return items
    end, { title = "Rewrite", selection = true, refocus = false })
  end

  -- With modes, the menu's chord is J, so the direct rewrite needs a chord
  -- of its own.
  local rewrite = option(opts.rewrite, not modes and "SUPER + ALT + J")
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
        local summary = ask(text, M.prompts.summarize, "Summarizing…"):trim()
        om.busy()
        om.notify("Summary", summary)
      end
    end)
  end

  local translate = option(opts.translate, "SUPER + ALT + T")
  if translate then
    local language = opts.language or "English"
    local system = string.format(M.prompts.translate, language)
    om.hotkey(translate, function()
      local text = selection()
      if text then
        om.paste(unquote(ask(text, system, "Translating to " .. language .. "…")))
      end
    end)
  end
  return M
end

return M
