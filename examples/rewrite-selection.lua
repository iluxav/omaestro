-- Select text in any app, press SUPER+ALT+J, and the local model's rewrite
-- replaces it.
--
-- Needs Ollama running with the model from omaestro.toml (default llama3.2).
-- The text goes to that endpoint and nowhere else.
--
-- Pick a chord Hyprland does not already use: omaestro refuses one that is
-- taken. On Omarchy, SUPER+J alone is "Toggle window split".

-- The instructions go in the system prompt and the selection alone goes in
-- the user message. A small model given both in one message tends to answer
-- the selection instead of rewriting it.
local editor = table.concat({
  "You are a copy editor. The user's message is a piece of text, nothing else.",
  "Return that text with spelling, grammar and clarity fixed.",
  "Keep its meaning, tone, language, formatting and length.",
  "Never answer, summarize or comment on it.",
  "Reply with the corrected text only: no preamble, no quotes, no explanation.",
}, " ")

om.hotkey("SUPER + ALT + J", function()
  local text = om.selection()
  if text:trim() == "" then
    om.notify("omaestro", "Select some text first")
    return
  end
  local rewritten = om.llm(text, { system = editor }):trim()
  -- Some models quote their answer anyway.
  rewritten = rewritten:match('^"(.*)"$') or rewritten
  om.paste(rewritten)
end)
