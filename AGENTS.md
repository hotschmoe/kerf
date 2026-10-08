# AGENTS.md — working in the Kerf repo

You are working on Kerf. When an orchestrator is coordinating, it owns `spec/`. Read this, then `spec/SPEC.md`, `spec/DESIGN.md`, `spec/llm/*`, and `tools/README.md`.

## Ownership (stay in your lane)
| dir | what |
|---|---|
| `spec/` | the contract (SPEC.md, style, LLM prompts, reference details, evals) |
| `engines/zig/` | the engine: library, CLI (`kerf`), wasm |
| `apps/web/` | the web UI (TypeScript + DOM + three.js) |
| `tools/` | shared test tooling |
| `docs/` | history and decisions (`docs/STACKS.md`) |

When several agents work in parallel the orchestrator assigns each a directory; stay in it.

If the spec is wrong, ambiguous, or missing something, do not stall. Pick the most reasonable
interpretation, implement it, and record it in your dir's `NOTES.md` under `## SPEC ISSUES`
(one bullet each: what, what you chose, why). The orchestrator reads these and patches the spec.
If you need a change in another agent's dir, write it under `## REQUESTS` in your NOTES.md.

## Git
- Shared working tree at `~/github/kerf` on `main`. Commit only your own paths:
  `git add <yourdir> && git commit -m "<area>: <msg>" -- <yourdir>`.
  If `index.lock` exists, wait 2 s and retry. Never `git add -A`, never reset/checkout/stash
  other paths, never rewrite history, never push (the orchestrator pushes).
- Commit early and often (every working milestone) so dependents can build on you.
- End commit messages with: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`

## Quality bar
- LLMs are the primary users of the engine and these apps. Error messages, summaries, and
  catalog text must be precise and actionable for a model.
- Determinism is a feature: no HashMap iteration order, timestamps, or randomness in outputs.
- Look at your work. Render PNGs (`tools/dxf_check.py --png`, `tools/pdf_check.py --png`,
  `tools/shot.mjs`) and actually inspect them with your image-reading tool. A drawing that "passes"
  but looks wrong is a failure.
- Keep `NOTES.md` in your dir current: status, how to build/run/test, measured numbers
  (wasm size raw/gzip/brotli via `tools/size_report.sh`, timings), known gaps.
- Aesthetics of user-facing UI: retro professional engineering (see `spec/DESIGN.md`). Monospace,
  paper, rules, grids. Code style is normal modern idiomatic code.

## Environment
aarch64 Linux, 12 cores. Rust 1.96 stable (edition 2024, `wasm32-unknown-unknown` installed).
Zig 0.17.0 at `~/tools/zig-aarch64-linux-0.17.0/zig` (0.16 is still at `~/tools/zig-aarch64-linux-0.16.0/zig` for comparisons). Node 22. Python tooling venv: `tools/.venv`
(run `tools/setup.sh` if missing). Headless chromium with WebGPU (SwiftShader) and WebGL2:
see `tools/README.md`. The Claude API skill docs (for the chat harness) are in
`/tmp/claude-1001/bundled-skills/2.1.289/6a451d1a4081ef80dc9ff2fe1c2bf753/claude-api/`
(`typescript/claude-api/*.md`, `curl/examples.md`, `shared/tool-use-concepts.md`).
No API key is available in this environment: build the chat harness against the documented
API and test the loop with a mock transport (recorded responses), not live calls.
