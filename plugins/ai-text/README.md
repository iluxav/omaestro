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
