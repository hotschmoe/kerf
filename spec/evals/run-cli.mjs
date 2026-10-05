#!/usr/bin/env node
// Kerf CLI-path eval runner: drives a coding agent (Claude Code headless, later Grok / Codex)
// through the real `kerf` CLI in a fresh workspace per prompt, then grades the final document.
// No dependencies (node builtins only); the engine CLI renders the PNGs.
//
//   node spec/evals/run-cli.mjs --agent claude [--kerf ~/kerf-eval/bin/kerf-baseline] [--only e01,e05]
//        [--model sonnet] [--out ~/kerf-eval/runs] [--timeout 1500]
//
// Each run uses the owner's agent subscription: every prompt runs ONCE, sequentially.
// Output: <out>/<timestamp>/summary.json and, per case, <id>/{workspace/, transcript.jsonl,
// digest.md, final.kerf.json, <id>-A.png, score.json}.
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");
const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => {
    if (a.startsWith("--")) acc.push([a.slice(2), all[i + 1] === undefined || all[i + 1].startsWith("--") ? true : all[i + 1]]);
    return acc;
  }, []),
);
const home = os.homedir();
const agentName = String(args.agent ?? "claude");
const kerfBin = path.resolve(String(args.kerf ?? path.join(home, "kerf-eval/bin/kerf-baseline")));
const outRoot = path.resolve(String(args.out ?? path.join(home, "kerf-eval/runs")));
const only = args.only ? new Set(String(args.only).split(",")) : null;
const timeoutMs = Number(args.timeout ?? 1500) * 1000;
const model = args.model && args.model !== true ? String(args.model) : null;

const style = JSON.parse(fs.readFileSync(path.join(root, "spec/styles/kerf-standard.kerfstyle.json"), "utf8"));
const APPEND = (c) =>
  c.start
    ? ` Work in this folder with the kerf CLI. Edit ${c.start}.kerf.json.`
    : ` Work in this folder with the kerf CLI. Name the file ${c.id}.kerf.json.`;

// ---- agent adapters -----------------------------------------------------------------------
// Each returns { cmd, argv } to spawn with cwd = workspace and stdin closed. `parse(events)`
// extracts { finalText, usage, cost, turns, tools: [{command, output, isError}] } best-effort.
const agents = {
  claude: {
    build: (msg) => ({
      cmd: "claude",
      argv: [
        "-p", msg, "--output-format", "stream-json", "--verbose",
        "--permission-mode", "acceptEdits",
        "--allowedTools", "Bash(kerf:*)", "Bash(kerf *)", "Bash(cat *)", "Bash(ls *)", "Bash(head *)", "Bash(tail *)", "Bash(grep *)", "Bash(jq *)", "Read", "Write", "Edit", "Glob", "Grep",
        ...(model ? ["--model", model] : []),
      ],
    }),
    parse(events) {
      const uses = new Map();
      const tools = [];
      let result = null;
      for (const ev of events) {
        if (ev.type === "assistant") {
          for (const b of ev.message?.content ?? []) {
            if (b.type === "tool_use") {
              const t = { name: b.name, command: b.name === "Bash" ? b.input?.command : JSON.stringify(b.input), output: "", isError: false };
              uses.set(b.id, t);
              tools.push(t);
            }
          }
        } else if (ev.type === "user") {
          for (const b of ev.message?.content ?? []) {
            if (b.type === "tool_result" && uses.has(b.tool_use_id)) {
              const t = uses.get(b.tool_use_id);
              t.output = typeof b.content === "string" ? b.content : (b.content ?? []).map((x) => x.text ?? `[${x.type}]`).join("\n");
              t.isError = !!b.is_error;
            }
          }
        } else if (ev.type === "result") result = ev;
      }
      return {
        finalText: result?.result ?? lastAssistantText(events),
        usage: result?.usage ?? null,
        cost: result?.total_cost_usd ?? null,
        turns: result?.num_turns ?? null,
        agentDurationMs: result?.duration_ms ?? null,
        isError: !!result?.is_error,
        model: events.find((e) => e.type === "system" && e.subtype === "init")?.model ?? null,
        tools,
      };
    },
  },
  // Templates: implemented but not yet run in anger. Event shapes are parsed best-effort.
  grok: {
    build: (msg, dir) => ({ cmd: "grok", argv: ["-p", msg, "--output-format", "streaming-json", "--always-approve", "--cwd", dir] }),
    parse: genericParse,
  },
  codex: {
    build: (msg, dir) => ({ cmd: "codex", argv: ["exec", "--sandbox", "workspace-write", "--skip-git-repo-check", "--cd", dir, "--json", msg] }),
    parse: genericParse,
  },
};

function lastAssistantText(events) {
  for (let i = events.length - 1; i >= 0; i--) {
    const c = events[i].message?.content;
    if (Array.isArray(c)) {
      const t = c.filter((b) => b.type === "text").map((b) => b.text).join("\n");
      if (t) return t;
    }
  }
  return "";
}

function genericParse(events) {
  // Best-effort: concatenate any text-ish fields; tool calls are counted from the kerf wrapper log.
  const texts = [];
  for (const ev of events) {
    for (const k of ["text", "message", "content", "result"]) if (typeof ev[k] === "string") texts.push(ev[k]);
    if (ev.item?.text) texts.push(ev.item.text);
  }
  return { finalText: texts.slice(-8).join("\n"), usage: null, cost: null, turns: null, agentDurationMs: null, isError: false, tools: [] };
}

// ---- grading ------------------------------------------------------------------------------
function kerfCall(fn, input) {
  return JSON.parse(execFileSync(kerfBin, ["call", fn], { input: JSON.stringify(input), maxBuffer: 64 << 20 }).toString("utf8"));
}

// Key-order-insensitive JSON (the engine canonicalizes key order on write).
const stable = (v) =>
  JSON.stringify(v, (_, x) => (x && typeof x === "object" && !Array.isArray(x) ? Object.fromEntries(Object.entries(x).sort(([a], [b]) => (a < b ? -1 : 1))) : x));

// Keys the spec (§6) defines on views / annotations. Anything else is silently kept and ignored by
// the engine, so a typo like "citations" for "cite" looks fine in `kerf check`. Reported as info.
const KNOWN = {
  view: ["id", "kind", "number", "title", "scale", "cut_z", "crop", "from", "cutaway", "notes_side", "omit", "annotations"],
  note: ["id", "type", "text", "target", "at", "place", "cite"],
  dim: ["id", "type", "from", "to", "dir", "offset", "text"],
  label: ["id", "type", "text", "at", "offset"],
};
function unknownKeys(doc) {
  const out = [];
  for (const v of doc.views ?? []) {
    for (const k of Object.keys(v)) if (!KNOWN.view.includes(k)) out.push(`view ${v.id}.${k}`);
    for (const a of v.annotations ?? []) for (const k of Object.keys(a)) if (!(KNOWN[a.type] ?? []).includes(k)) out.push(`${a.id}.${k}`);
  }
  return out;
}

function grade(c, doc, startDoc, finalText) {
  const e = c.expect ?? {};
  const checks = [];
  const check = (name, pass, detail = "") => checks.push({ name, pass: !!pass, detail });
  const types = new Set(doc.components.map((x) => x.type));
  const notes = doc.views.flatMap((v) => v.annotations ?? []).filter((a) => a.type === "note");
  const noteText = notes.map((n) => n.text).join("\n").toUpperCase();
  for (const t of e.types ?? []) check(`has type ${t}`, types.has(t));
  for (const t of e.must_not_types ?? []) check(`no type ${t}`, !types.has(t));
  if (e.min_notes) check(`>= ${e.min_notes} notes`, notes.length >= e.min_notes, `${notes.length}`);
  for (const k of e.views ?? []) check(`has ${k} view`, doc.views.some((v) => (v.kind ?? "section") === k));
  for (const re of e.must_mention ?? []) check(`mentions /${re}/`, new RegExp(re, "i").test(noteText));
  for (const id of e.removed ?? []) check(`removed ${id}`, !doc.components.some((x) => x.id === id));
  if (e.changed_only && startDoc) {
    const byId = new Map(startDoc.components.map((x) => [x.id, stable(x)]));
    const changed = doc.components.filter((x) => byId.has(x.id) && byId.get(x.id) !== stable(x)).map((x) => x.id);
    const gone = startDoc.components.filter((x) => !doc.components.some((y) => y.id === x.id)).map((x) => x.id);
    const bad = [...changed, ...gone].filter((id) => !e.changed_only.includes(id));
    check("changed_only", bad.length === 0, bad.length ? `also changed/removed: ${bad.join(",")}` : `changed: ${changed.join(",")}`);
  }
  if (e.asks_question_or_states_assumptions) check("question/assumptions", /\?|assum/i.test(finalText));

  let diag = [];
  let checkError = null;
  try {
    diag = kerfCall("check", { doc, style }).diagnostics ?? [];
  } catch (err) {
    checkError = String(err.stderr ?? err.message ?? err).slice(0, 300);
  }
  const errors = diag.filter((d) => d.level === "error");
  const warns = diag.filter((d) => d.level === "warning");
  check("zero errors", !checkError && errors.length === 0, checkError ?? errors.map((d) => d.code).join(","));
  const cites = notes.flatMap((n) => n.cite ?? []);
  check("all citations suggested", cites.every((x) => x.status === "suggested"), `${cites.length} cites`);
  // score_v1 = the original formula (expect checks + errors + citations, minus 0.02 per warning),
  // kept so runs stay comparable with the 2026-10-06 baseline.
  const passedV1 = checks.filter((x) => x.pass).length;
  const scoreV1 = +(passedV1 / checks.length - 0.02 * warns.length).toFixed(3);
  // Quality gates (only for docs with views): things the expect checks cannot see.
  const count = (code) => diag.filter((d) => d.code === code).length;
  const unknown = unknownKeys(doc);
  const GATE_CODES = ["W_VIEW_FIT", "W_LEADER_HIT", "W_NOTE_TARGET", "W_UNKNOWN_KEY"];
  if ((doc.views ?? []).length) {
    check("gate: no unknown keys", unknown.length === 0 && count("W_UNKNOWN_KEY") === 0, unknown.join(", "));
    check("gate: W_VIEW_FIT == 0", count("W_VIEW_FIT") === 0);
    check("gate: no leader hits/crossings (W_LEADER_HIT)", count("W_LEADER_HIT") === 0, `${count("W_LEADER_HIT")}`);
    check("gate: every note target visible (W_NOTE_TARGET)", count("W_NOTE_TARGET") === 0, `${count("W_NOTE_TARGET")}`);
  }
  const passed = checks.filter((x) => x.pass).length;
  const penalised = warns.filter((d) => !GATE_CODES.includes(d.code)).length;
  return {
    id: c.id,
    score: +(passed / checks.length - 0.02 * penalised).toFixed(3),
    score_v1: scoreV1,
    passed,
    total: checks.length,
    warnings: warns.length,
    warning_codes: warns.map((d) => d.code),
    errors: errors.map((d) => `${d.code}: ${d.message}`),
    notes: notes.length,
    citations: cites.length,
    unknown_keys: unknown,
    checks,
  };
}

const SANDBOX_RE = /requires approval|require approval|was blocked|can't be checked|obfuscation|Brace expansion|haven't granted|needs approval|outside the working|permission/i;

// ---- running ------------------------------------------------------------------------------
function findFinalDoc(dir, c) {
  const want = path.join(dir, `${c.start ?? c.id}.kerf.json`);
  if (fs.existsSync(want)) return want;
  const others = fs.readdirSync(dir).filter((f) => f.endsWith(".kerf.json"));
  return others.length ? path.join(dir, others[0]) : null;
}

function runAgent(spec, dir, env) {
  return new Promise((resolve) => {
    const t0 = Date.now();
    const child = spawn(spec.cmd, spec.argv, { cwd: dir, env, stdio: ["ignore", "pipe", "pipe"] });
    let out = "", err = "", timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGTERM"); }, timeoutMs);
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (code) => { clearTimeout(timer); resolve({ out, err, code, timedOut, wallMs: Date.now() - t0 }); });
    child.on("error", (e) => { clearTimeout(timer); resolve({ out, err: String(e), code: -1, timedOut, wallMs: Date.now() - t0 }); });
  });
}

function digest(tools, finalText) {
  const lines = [];
  tools.forEach((t, i) => {
    lines.push(`## ${i + 1}. ${t.name}${t.isError ? "  [ERROR]" : ""}`, "```", String(t.command ?? "").slice(0, 3000), "```");
    if (t.output) lines.push("output:", "```", t.output.slice(0, 1500), "```");
  });
  lines.push("## final message", finalText);
  return lines.join("\n");
}

async function runCase(c, runDir, agent) {
  const outDir = path.join(runDir, c.id);
  const ws = path.join(outDir, "workspace");
  // --recover <runDir>: skip the agent, re-process the saved transcript and workspace of a case
  // whose post-processing crashed (never re-runs the agent).
  const recovering = !!args.recover && fs.existsSync(path.join(outDir, "transcript.jsonl"));
  let startDoc = null;
  if (!recovering) {
    fs.mkdirSync(ws, { recursive: true });
    execFileSync(path.join(runDir, "bin/kerf"), ["init"], { cwd: ws, stdio: "ignore" });
    if (c.start) fs.copyFileSync(path.join(root, "spec/details", `${c.start}.kerf.json`), path.join(ws, `${c.start}.kerf.json`));
  }
  if (c.start) startDoc = JSON.parse(fs.readFileSync(path.join(root, "spec/details", `${c.start}.kerf.json`), "utf8"));
  let msg = c.prompt;
  if (c.attach) {
    const name = path.basename(c.attach);
    if (!recovering) fs.copyFileSync(path.join(root, c.attach), path.join(ws, name));
    msg += ` The screenshot is ./${name} (read it).`;
  }
  msg += APPEND(c);

  const logFile = path.join(outDir, "kerf-calls.log");
  const env = { ...process.env, PATH: `${path.join(runDir, "bin")}:${process.env.PATH}`, KERF_EVAL_LOG: logFile };
  const run = recovering
    ? { out: fs.readFileSync(path.join(outDir, "transcript.jsonl"), "utf8"), err: "", code: null, timedOut: false, wallMs: null }
    : await runAgent(agent.build(msg, ws), ws, env);
  const events = run.out.split("\n").filter(Boolean).map((l) => { try { return JSON.parse(l); } catch { return { type: "raw", text: l }; } });
  fs.writeFileSync(path.join(outDir, "transcript.jsonl"), run.out);
  if (run.err) fs.writeFileSync(path.join(outDir, "stderr.txt"), run.err);
  const parsed = agent.parse(events);
  const prior = recovering && fs.existsSync(path.join(outDir, "score.json")) ? JSON.parse(fs.readFileSync(path.join(outDir, "score.json"), "utf8")) : null;
  fs.writeFileSync(path.join(outDir, "digest.md"), `# ${c.id}\n\nPROMPT: ${msg}\n\n` + digest(parsed.tools, parsed.finalText));

  // Log lines are "<epoch>\t<args>"; args may span lines, so only lines starting with an epoch count.
  const entries = fs.existsSync(logFile)
    ? fs.readFileSync(logFile, "utf8").split(/\n(?=\d{10}\t)/).filter(Boolean).map((l) => [+l.slice(0, 10), l.slice(11).trim()])
    : [];
  const calls = entries.map((x) => x[1]);
  // Discovery vs building: a "probe" is a raw-API call or an apply that writes nothing (scratch -o / --dry-run).
  const isWrite = (k) => /^apply\b/.test(k) && /(^|\s)(-w|--write)(\s|$)/.test(k);
  const isProbe = (k) => /^call\b/.test(k) || (/^apply\b/.test(k) && !isWrite(k));
  const fw = entries.findIndex((x) => isWrite(x[1]));
  const callStats = {
    first_write_call: fw >= 0 ? fw + 1 : null,
    probe_calls_before_first_write: (fw >= 0 ? calls.slice(0, fw) : calls).filter(isProbe).length,
    probe_calls_total: calls.filter(isProbe).length,
    schema_calls: calls.filter((k) => /^schema\b/.test(k)).length,
    first_write_s: fw >= 0 ? entries[fw][0] - entries[0][0] : null,
  };
  const byVerb = {};
  for (const k of calls) { const v = k.split(/\s+/)[0]; byVerb[v] = (byVerb[v] ?? 0) + 1; }
  const meta = {
    wall_s: run.wallMs != null ? +(run.wallMs / 1000).toFixed(1) : prior?.wall_s ?? (parsed.agentDurationMs ? +(parsed.agentDurationMs / 1000).toFixed(1) : null),
    wall_source: run.wallMs != null ? "runner" : prior?.wall_s ? prior.wall_source ?? "runner" : "agent-reported", agent_s: parsed.agentDurationMs ? +(parsed.agentDurationMs / 1000).toFixed(1) : null,
    kerf_calls: calls.length, kerf_by_verb: byVerb, tool_errors: parsed.tools.filter((t) => t.isError).length,
    ...callStats,
    // tool errors caused by the headless permission sandbox rather than the engine (pipes, ';', /tmp, WebFetch ...)
    sandbox_blocked: parsed.tools.filter((t) => t.isError && SANDBOX_RE.test(t.output)).length,
    engine_errors: parsed.tools.filter((t) => t.isError && !SANDBOX_RE.test(t.output)).length,
    engine_error_samples: parsed.tools.filter((t) => t.isError && !SANDBOX_RE.test(t.output)).slice(0, 4).map((t) => `${String(t.command).slice(0, 80)} -> ${t.output.slice(0, 160)}`),
    turns: parsed.turns, cost_usd: parsed.cost, usage: parsed.usage, exit_code: run.code, timed_out: run.timedOut,
    agent_is_error: parsed.isError,
    model: parsed.model ?? null,
  };

  const finalPath = findFinalDoc(ws, c);
  let result;
  if (!finalPath && c.expect?.asks_question_or_states_assumptions) {
    // Asking instead of building is a valid outcome for an ambiguous prompt: grade the reply text only.
    result = grade({ ...c, expect: { asks_question_or_states_assumptions: true } }, { components: [], views: [] }, null, parsed.finalText);
    result.no_doc = true;
  } else if (!finalPath) {
    result = { id: c.id, score: 0, passed: 0, total: 0, warnings: 0, error: "no .kerf.json produced", checks: [] };
  } else {
    const doc = JSON.parse(fs.readFileSync(finalPath, "utf8"));
    fs.writeFileSync(path.join(outDir, "final.kerf.json"), JSON.stringify(doc, null, 2));
    result = grade(c, doc, startDoc, parsed.finalText);
    result.final_file = path.basename(finalPath);
    // The engine's own check summary, for the report.
    try { result.check_summary = kerfCall("check", { doc, style }).summary?.split("\n")[0]; } catch {}
    const view = doc.views.find((v) => v.id === "A") ?? doc.views[0];
    if (view) {
      const png = path.join(outDir, `${c.id}-${view.id}.png`);
      try {
        execFileSync(kerfBin, ["export", finalPath, "--view", view.id, "--format", "png", "--px", "1600", "-o", png], { stdio: "pipe" });
        result.png = path.relative(runDir, png);
      } catch (e) { result.png_error = String(e.stderr ?? e.message).slice(0, 200); }
    }
  }
  result.final_message = parsed.finalText?.slice(0, 2000);
  Object.assign(result, meta);
  fs.writeFileSync(path.join(outDir, "score.json"), JSON.stringify(result, null, 2));
  return result;
}

// --regrade <runDir>: re-grade the saved final docs of an earlier run (no agent calls).
if (args.regrade) {
  const dir = path.resolve(String(args.regrade));
  const all = fs.readFileSync(path.join(here, "prompts.jsonl"), "utf8").trim().split("\n").map((l) => JSON.parse(l));
  const sum = JSON.parse(fs.readFileSync(path.join(dir, "summary.json"), "utf8"));
  for (const r of sum.results) {
    const c = all.find((x) => x.id === r.id);
    const f = path.join(dir, r.id, "final.kerf.json");
    if (!c || !fs.existsSync(f)) continue;
    const startDoc = c.start ? JSON.parse(fs.readFileSync(path.join(root, "spec/details", `${c.start}.kerf.json`), "utf8")) : null;
    const g = grade(c, JSON.parse(fs.readFileSync(f, "utf8")), startDoc, r.final_message ?? "");
    Object.assign(r, { score: g.score, score_v1: g.score_v1, unknown_keys: g.unknown_keys, passed: g.passed, total: g.total, warnings: g.warnings, warning_codes: g.warning_codes, errors: g.errors, checks: g.checks });
    fs.writeFileSync(path.join(dir, r.id, "score.json"), JSON.stringify(r, null, 2));
    console.log(`${r.id} ${g.passed}/${g.total} warn ${g.warnings} score ${g.score}`);
  }
  fs.writeFileSync(path.join(dir, "summary.json"), JSON.stringify(sum, null, 2));
  process.exit(0);
}

const agent = agents[agentName];
if (args.recover) {
  const dir = path.resolve(String(args.recover));
  const sumPath = path.join(dir, "summary.json");
  const sum = JSON.parse(fs.readFileSync(sumPath, "utf8"));
  const all = fs.readFileSync(path.join(here, "prompts.jsonl"), "utf8").trim().split("\n").map((l) => JSON.parse(l));
  for (const c of all.filter((x) => !only || only.has(x.id.split("-")[0]) || only.has(x.id))) {
    const r = await runCase(c, dir, agent);
    const i = sum.results.findIndex((x) => x.id === c.id);
    if (i >= 0) sum.results[i] = r; else sum.results.push(r);
    console.log(`${c.id} recovered: ${r.passed}/${r.total} warn ${r.warnings} score ${r.score}`);
  }
  fs.writeFileSync(sumPath, JSON.stringify(sum, null, 2));
  process.exit(0);
}
if (!agent) { console.error(`unknown --agent ${agentName} (claude|grok|codex)`); process.exit(2); }
const cases = fs.readFileSync(path.join(here, "prompts.jsonl"), "utf8").trim().split("\n").map((l) => JSON.parse(l))
  .filter((c) => !only || only.has(c.id.split("-")[0]) || only.has(c.id));
const runDir = path.join(outRoot, new Date().toISOString().replace(/[:.]/g, "-"));
fs.mkdirSync(path.join(runDir, "bin"), { recursive: true });
// `kerf` shim first on PATH: pins the engine under test and logs every invocation for counting.
fs.writeFileSync(
  path.join(runDir, "bin/kerf"),
  `#!/bin/sh\n[ -n "$KERF_EVAL_LOG" ] && printf '%s\\t%s\\n' "$(date +%s)" "$*" >> "$KERF_EVAL_LOG"\nexec "${kerfBin}" "$@"\n`,
  { mode: 0o755 },
);
const kerfVersion = execFileSync(kerfBin, ["version"]).toString().trim();
console.log(`agent ${agentName}  engine ${kerfBin} ${kerfVersion}\nrun dir ${runDir}`);

const results = [];
for (const c of cases) {
  process.stdout.write(`${c.id} … `);
  try {
    const r = await runCase(c, runDir, agent);
    results.push(r);
    console.log(`${r.passed}/${r.total} warn ${r.warnings} score ${r.score}  ${r.wall_s}s  kerf×${r.kerf_calls}  $${r.cost_usd ?? "?"}${r.timed_out ? "  TIMEOUT" : ""}`);
  } catch (e) {
    console.log(`ERROR ${e.message}`);
    results.push({ id: c.id, score: 0, error: String(e.message) });
  }
  fs.writeFileSync(path.join(runDir, "summary.json"), JSON.stringify({ agent: agentName, model, engine: kerfBin, engine_version: kerfVersion, results }, null, 2));
}
const mean = results.reduce((s, r) => s + r.score, 0) / (results.length || 1);
console.log(`mean score ${mean.toFixed(3)}  → ${runDir}`);
