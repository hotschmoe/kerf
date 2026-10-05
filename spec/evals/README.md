# Kerf LLM evals

These prompts measure how reliably Claude builds correct, consistent details through the Kerf
tool surface. They are the bake-off's "LLM drives the engine" metric, and later they become a
regression suite for prompt and catalog changes.

Each row of `prompts.jsonl` has:
- `prompt` (and optionally `start`, a reference detail to begin from, and `attach`, an image)
- `expect` checks that are graded automatically from the final document:
  - `types`: component types that must be present
  - `must_not_types`: component types that must be absent
  - `min_notes`: minimum number of note annotations
  - `views`: view kinds that must be present
  - `must_mention`: regexes that must appear across the note text (`|` means alternatives)
  - `changed_only` / `removed`: for edit tasks, which ids may change or must disappear
  - `asks_question_or_states_assumptions`: for ambiguous prompts

Every final document must also have **zero errors**. Every **warning is counted against the
score**, and every citation must have `status: "suggested"`.

Human or LLM-judge rubric (score each 0–2):
1. Reads as the detail a US structural engineer expects.
2. Members are correctly sized and positioned, with no visual overlaps or gaps.
3. Note grammar follows `spec/llm/system.md`, and the notes are consistent across details.
4. Citations are plausible and none are invented. Uncertainty is surfaced.

Run (needs `ANTHROPIC_API_KEY` and a built engine CLI):
```
node spec/evals/run.mjs --engine engines/zig/zig-out/bin/kerf --model claude-opus-5-5 [--only e01]
```
The runner drives the same tool loop as the apps (spec/llm/HARNESS.md). It renders
`kerf_render` images through the engine's SVG output and headless chromium. Each run writes
`spec/evals/runs/<timestamp>/<id>/` containing the final doc, transcript, PNGs and score.json.
