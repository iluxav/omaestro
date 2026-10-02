# ai-text

The selection through the local model: rewrite it in place, show a summary,
or replace it with a translation. Select text in any app, press the chord,
and the result lands where the text was. Needs Ollama running with the
model from `omaestro.toml` (default `llama3.2`); the text goes to that
endpoint and nowhere else.

| Chord | What |
|---|---|
| SUPER+ALT+J | replaces the selection with a corrected version (spelling, grammar, clarity; same meaning and length) |
| SUPER+ALT+M | shows a three-sentence summary as a notification; the text stays |
| SUPER+ALT+T | replaces the selection with its translation (English by default) |

## Install

```sh
om plugin add ai-text
```

It installs with its defaults and lists them; `om plugin configure ai-text`
opens them as a form in your editor. Either way they are written to
`~/.config/omaestro/rules.d/ai-text.lua`, which you can also edit.

That writes `~/.config/omaestro/rules.d/ai-text.lua`, which is where the
options go:

```lua
local ai = om.use("ai-text")
ai.setup({ language = "German", summarize = false })
```

## Options

- `rewrite`, `summarize`, `translate`: the chords; `false` switches one off.
- `language`: the translation target (default `English`).
- `model`: a model name for all three; default from `omaestro.toml`.
- The prompts are in `ai.prompts` and can be changed before `setup`.

## Modes: one chord, a menu

Turn on `modes` and SUPER+ALT+J asks which mode to apply instead of always
correcting. Type to filter, Enter picks, Escape cancels. The last mode used
is first, so the chord and Enter repeat it.

```lua
om.use("ai-text").setup({ modes = true })   -- the modes in ai.modes
```

Or your own list, in the order the menu shows it:

```lua
om.use("ai-text").setup({
  modes = {
    { "Fix English only", "Fix spelling and grammar. Change nothing else." },
    { "More polite",      "Rewrite it so it is polite and warm. Keep the meaning." },
    { "Marketing pitch",  "Rewrite it as a short, confident marketing pitch." },
    { "Summarize",        "Summarize it in three sentences.", show = true },
  },
})
```

- Each mode is a label and an instruction. The instruction is wrapped in
  `ai.frame`, which tells the model to work on the text and reply with the
  result only, so a short instruction is enough.
- `show = true` shows the answer as a notification and leaves the text alone.
- `custom` (default `true`) adds a last row, "Custom instruction...", that
  asks for a one-off instruction.
- `menu`: the chord that opens the menu (default `SUPER + ALT + J`). With
  modes on, the direct rewrite chord is off unless `rewrite` names another
  chord; summarize and translate stay as they are.
- The selection and the focused window are read before the menu opens, and
  that window gets the focus back before the paste.
- While the model works, a notification says so ("Rewriting…", the mode's
  name); it goes away when the answer is in.
- A selection made in another window, or one already rewritten, counts as
  none: the chord asks you to select some text instead of rewriting old
  text.
