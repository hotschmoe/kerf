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

## CLI-path eval (`run-cli.mjs`): a coding agent driving the real `kerf` CLI

`run.mjs` above exercises the raw API tool loop. `run-cli.mjs` measures the path users actually
take: a headless coding agent in a fresh workspace folder, using only the `kerf` CLI
(`kerf init` → `kerf guide` → `kerf apply -w` → `kerf export`).

```
node spec/evals/run-cli.mjs --agent claude [--kerf ~/kerf-eval/bin/kerf-baseline] [--only e01,e05] \
     [--model sonnet] [--out ~/kerf-eval/runs] [--timeout 1500]
```
- `--agent claude` runs `claude -p … --output-format stream-json --verbose --permission-mode acceptEdits
  --allowedTools "Bash(kerf:*)" "Bash(kerf *)" Read Write Edit`. `grok` and `codex` adapters are
  templated (`grok -p … --output-format streaming-json --always-approve --cwd <dir>`,
  `codex exec --sandbox workspace-write --skip-git-repo-check --cd <dir> --json <msg>`) but have only
  best-effort result parsing so far.
- `--kerf` pins the engine under test. The runner puts a `kerf` shim for it first on the agent's PATH;
  the shim also logs each invocation (the "kerf calls" metric). Use a frozen copy of the binary
  when other work is changing the engine.
- Each prompt runs once, sequentially, in `<out>/<ts>/<id>/workspace/` (seeded with `kerf init`, the
  `start` detail, or the `attach` image). The runner appends "Work in this folder with the kerf CLI.
  Name the file <id>.kerf.json (or edit <start>.kerf.json)."
- Grading uses the same `expect` checks as above plus: `kerf check` has 0 errors, every warning costs
  0.02, all citations are `suggested`, and four quality gates (no unknown view/annotation keys, no `W_VIEW_FIT`,
  no `W_LEADER_HIT`, no `W_NOTE_TARGET`). `score_v1` in `score.json` is the original formula, kept for comparing
  with older runs. The gates replace the per-warning penalty for their own codes. `changed_only` is checked on components (any component of the
  start doc that changed or vanished must be in the list).
- Output per case: `transcript.jsonl` (raw agent stream), `digest.md` (every tool call + output),
  `final.kerf.json`, `<id>-A.png` (view A rendered by the engine; **look at it**, then apply the
  0-2 rubric above by hand), `score.json`; plus `summary.json` for the run.
- `score.json` also records `unknown_keys` (view/annotation keys the spec does not define, which the engine
  silently keeps), `kerf_calls` by verb, `tool_errors` and `sandbox_blocked` (errors caused by the headless
  permission sandbox, not the engine), `probe_calls_before_first_write` / `first_write_call` / `first_write_s`
  (schema discovery before the first `apply -w`), tokens, cost and the agent's final message.
- `--regrade <runDir>` re-grades saved final docs; `--recover <runDir> [--only id]` rebuilds a case from its saved
  transcript and workspace if post-processing crashed. Neither re-runs the agent.
- Results are written up in `spec/evals/results/` (first one: `2026-10-06-claude-code-baseline.md`).

### Geometry checks and the visual judge

- `geom.mjs` computes independent checks from `kerf call drawing` (not from engine warnings): leader/leader crossings,
  leaders through other text, dim text overlapping text/lines or sitting on outlines, overlapping annotation text,
  sloped panels ending short of their host truss (crop-aware: a member clipped by the crop is not "short"), and strap
  overhang past a beam face/joint (min 6"). Text widths are estimated (0.88 x cap height per character). They are added to
  `checks` as `geom: <metric> == 0` gates and stored under `geometry` in `score.json`.
- `judge.mjs` is a visual judge: one `claude -p` call per case (`--tools Read`, JSON output, default model, ~$0.10), given
  the PNG, the request, the engine summary, the notes and citations and the agent's report, scoring the 4 rubric criteria
  0-2 with one-line reasons. `node spec/evals/judge.mjs --run <runDir> --kerf <bin>` judges a saved run;
  `run-cli.mjs --judge` does it inline. `--hand spec/evals/calibration/hand-scores.json --hand-key alpha4` prints agreement
  with the author's hand scores (never shown to the judge).
- Calibration (judge prompt v3, 19 saved PNGs: baseline 9 + alpha4 10): mean abs diff per criterion 0.25 (baseline) and
  0.15 (alpha4) on the 0-2 scale; totals within +-1 of the hand total on 89% / 80% of cases; mean total 6.1 -> 7.2 for the
  judge vs 6.7 -> 7.7 by hand (same before/after improvement); judge bias -0.6 (slightly stricter than the author, mostly
  on r2: it flags minor leader/dimension clashes). Per-case correlation is only 0.4-0.5 because the cases cluster at 6-8:
  use the judge for run-to-run and engine-to-engine comparison, not to rank two cases within a run. The prompt was tuned on
  these same 19 cases (3 iterations), so expect slightly worse agreement on new ones.
