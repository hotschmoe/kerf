#!/usr/bin/env node
// Kerf LLM eval runner. Drives the same tool loop as the apps (spec/llm/HARNESS.md) against a
// Kerf engine CLI, then grades each final document against spec/evals/prompts.jsonl.
//
//   cd spec/evals && npm install
//   node run.mjs --engine ../../engines/zig/zig-out/bin/kerf [--model claude-opus-5-5] [--only e01,e05]
//
// Needs ANTHROPIC_API_KEY (or an `ant auth login` profile). Every run spends real money.
import Anthropic from "@anthropic-ai/sdk";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");
const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => {
    if (a.startsWith("--")) acc.push([a.slice(2), all[i + 1]?.startsWith("--") ? true : all[i + 1] ?? true]);
    return acc;
  }, []),
);
const enginePath = path.resolve(args.engine ?? path.join(root, "engines/zig/zig-out/bin/kerf"));
const model = args.model ?? "claude-opus-5-5";
const only = args.only ? new Set(String(args.only).split(",")) : null;
const maxRounds = 25;

const style = JSON.parse(fs.readFileSync(path.join(root, "spec/styles/kerf-standard.kerfstyle.json"), "utf8"));
const tools = JSON.parse(fs.readFileSync(path.join(root, "spec/llm/tools.json"), "utf8"));
const systemText =
  fs.readFileSync(path.join(root, "spec/llm/system.md"), "utf8") +
  "\n\n# Component catalog\n" +
  engineRaw("catalog", { format: "markdown" }).toString("utf8");

function engineRaw(fn, input) {
  return execFileSync(enginePath, ["call", fn], { input: JSON.stringify(input), maxBuffer: 64 << 20 });
}
function engine(fn, input) {
  return JSON.parse(engineRaw(fn, input).toString("utf8"));
}

const emptyDoc = { kerf: "0.1", id: "untitled", title: "", meta: {}, run: [-24, 24], components: [], views: [] };

async function renderPng(doc, view) {
  const svg = engineRaw("export", { doc, style, view, format: "svg" }).toString("utf8");
  const puppeteer = (await import(path.join(root, "tools/node_modules/puppeteer-core/lib/esm/puppeteer/puppeteer-core.js"))).default;
  const browser = await puppeteer.launch({ executablePath: "/usr/bin/chromium", headless: "new", args: ["--no-sandbox"] });
  try {
    const page = await browser.newPage();
    await page.setViewport({ width: 1400, height: 1000 });
    await page.setContent(`<html><body style="margin:0;background:#fff">${svg}</body></html>`);
    const el = await page.$("svg");
    return await el.screenshot({ type: "png", encoding: "base64" });
  } finally {
    await browser.close();
  }
}

async function runCase(client, c, outDir) {
  let doc = c.start
    ? JSON.parse(fs.readFileSync(path.join(root, "spec/details", `${c.start}.kerf.json`), "utf8"))
    : structuredClone(emptyDoc);
  const userContent = [];
  if (c.attach && fs.existsSync(path.join(root, c.attach))) {
    userContent.push({
      type: "image",
      source: { type: "base64", media_type: "image/png", data: fs.readFileSync(path.join(root, c.attach)).toString("base64") },
    });
  }
  userContent.push({ type: "text", text: c.prompt });
  const messages = [{ role: "user", content: userContent }];
  const log = [];
  let rounds = 0;
  let finalText = "";
  for (;;) {
    const resp = await client.beta.messages.create({
      model,
      max_tokens: 32000,
      thinking: { type: "adaptive" },
      output_config: { effort: "high" },
      betas: ["server-side-fallback-2026-07-01"],
      fallbacks: "default",
      system: [{ type: "text", text: systemText, cache_control: { type: "ephemeral" } }],
      tools,
      messages,
    });
    messages.push({ role: "assistant", content: resp.content });
    log.push({ stop_reason: resp.stop_reason, usage: resp.usage });
    finalText = resp.content.filter((b) => b.type === "text").map((b) => b.text).join("\n");
    if (resp.stop_reason !== "tool_use" || ++rounds > maxRounds) break;
    const results = [];
    for (const block of resp.content) {
      if (block.type !== "tool_use") continue;
      try {
        if (block.name === "kerf_apply") {
          const out = engine("apply", { doc, style, ops: block.input.ops, actor: "llm" });
          if (out.ok) doc = out.doc;
          results.push({ type: "tool_result", tool_use_id: block.id, is_error: !out.ok,
            content: JSON.stringify({ ok: out.ok, summary: out.summary, diagnostics: out.diagnostics }) });
        } else if (block.name === "kerf_inspect") {
          const out = engine("inspect", { doc, style, query: block.input });
          results.push({ type: "tool_result", tool_use_id: block.id, content: typeof out === "string" ? out : JSON.stringify(out) });
        } else if (block.name === "kerf_render") {
          const png = await renderPng(doc, block.input.view);
          fs.writeFileSync(path.join(outDir, `render-${rounds}-${block.input.view}.png`), Buffer.from(png, "base64"));
          results.push({ type: "tool_result", tool_use_id: block.id, content: [
            { type: "image", source: { type: "base64", media_type: "image/png", data: png } },
            { type: "text", text: `view ${block.input.view} rendered` },
          ] });
        } else {
          results.push({ type: "tool_result", tool_use_id: block.id, is_error: true, content: `unknown tool ${block.name}` });
        }
      } catch (e) {
        results.push({ type: "tool_result", tool_use_id: block.id, is_error: true, content: String(e.stderr ?? e.message ?? e) });
      }
    }
    messages.push({ role: "user", content: results });
  }
  fs.writeFileSync(path.join(outDir, "final.kerf.json"), JSON.stringify(doc, null, 2));
  fs.writeFileSync(path.join(outDir, "transcript.json"), JSON.stringify({ messages, log }, null, 2));
  return grade(c, doc, finalText);
}

function grade(c, doc, finalText) {
  const e = c.expect ?? {};
  const checks = [];
  const types = new Set(doc.components.map((x) => x.type));
  const notes = doc.views.flatMap((v) => v.annotations ?? []).filter((a) => a.type === "note");
  const noteText = notes.map((n) => n.text).join("\n").toUpperCase();
  const check = (name, pass, detail = "") => checks.push({ name, pass: !!pass, detail });
  for (const t of e.types ?? []) check(`has type ${t}`, types.has(t));
  for (const t of e.must_not_types ?? []) check(`no type ${t}`, !types.has(t));
  if (e.min_notes) check(`>= ${e.min_notes} notes`, notes.length >= e.min_notes, `${notes.length}`);
  for (const k of e.views ?? []) check(`has ${k} view`, doc.views.some((v) => v.kind === k));
  for (const re of e.must_mention ?? []) check(`mentions /${re}/`, new RegExp(re).test(noteText));
  for (const id of e.removed ?? []) check(`removed ${id}`, !doc.components.some((x) => x.id === id));
  if (e.asks_question_or_states_assumptions) check("question/assumptions", /\?|assum/i.test(finalText));
  const diag = engine("check", { doc, style }).diagnostics ?? [];
  check("zero errors", !diag.some((d) => d.level === "error"), diag.filter((d) => d.level === "error").map((d) => d.code).join(","));
  const warnings = diag.filter((d) => d.level === "warning").length;
  const cites = notes.flatMap((n) => n.cite ?? []);
  check("all citations suggested", cites.every((x) => x.status === "suggested"));
  const passed = checks.filter((x) => x.pass).length;
  return { id: c.id, score: passed / checks.length - 0.02 * warnings, passed, total: checks.length, warnings, checks };
}

const cases = fs.readFileSync(path.join(here, "prompts.jsonl"), "utf8").trim().split("\n").map((l) => JSON.parse(l))
  .filter((c) => !only || only.has(c.id.split("-")[0]) || only.has(c.id));
const client = new Anthropic();
const runDir = path.join(here, "runs", new Date().toISOString().replace(/[:.]/g, "-"));
const results = [];
for (const c of cases) {
  const outDir = path.join(runDir, c.id);
  fs.mkdirSync(outDir, { recursive: true });
  process.stdout.write(`${c.id} … `);
  try {
    const r = await runCase(client, c, outDir);
    fs.writeFileSync(path.join(outDir, "score.json"), JSON.stringify(r, null, 2));
    results.push(r);
    console.log(`${r.passed}/${r.total}  warn ${r.warnings}  score ${r.score.toFixed(2)}`);
  } catch (e) {
    console.log(`ERROR ${e.message}`);
    results.push({ id: c.id, score: 0, error: String(e.message) });
  }
}
fs.writeFileSync(path.join(runDir, "summary.json"), JSON.stringify({ model, engine: enginePath, results }, null, 2));
console.log(`mean score ${(results.reduce((s, r) => s + r.score, 0) / results.length).toFixed(3)}  → ${runDir}`);
