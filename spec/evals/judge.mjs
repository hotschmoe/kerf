#!/usr/bin/env node
// Visual LLM judge for Kerf eval runs: one headless `claude -p` call per case, Read tool only.
// It looks at the final PNG and scores the 4 rubric criteria (0-2 each) with one-line reasons.
//
//   node spec/evals/judge.mjs --run ~/kerf-eval/runs/<ts> [--only e01,e05] [--kerf <binary>]
//        [--hand spec/evals/calibration/hand-scores.json --hand-key alpha4] [--model sonnet] [--force]
//
// Writes <run>/<id>/judge.json and <run>/judge-summary.json. With --hand, also prints the agreement
// against the human scores (mean abs diff per criterion, exact-match rate, bias, total correlation).
// The judge never sees the hand scores. Cost is ~one small call per case (image + ~2 KB of text).
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");

export const CRITERIA = ["r1", "r2", "r3", "r4"];

export const JUDGE_SYSTEM = `You are a strict US structural-engineering reviewer scoring one AI-generated construction detail drawing against a request. Be skeptical: look for what is wrong, not what is right. Output ONLY one JSON object, no prose, no markdown fences.`;

export const RUBRIC = `Score each criterion 0, 1 or 2 on this scale:
2 = fit for a designer to review as-is: at most cosmetic nitpicks that a busy engineer would not mark up.
1 = at least one clear defect that a reviewer would mark up (name it).
0 = the criterion is clearly failed (wrong detail, pervasive defects, invented content).
Do not withhold a 2 for subjective taste, for things the request did not ask for, or for the drawing style of the CAD engine (stroke font, hatch patterns, thin hairlines for hardware, break lines at the crop edge). Do not give a 1 without naming a specific, visible, checkable defect.

r1 READS AS THE EXPECTED DETAIL. Would a US structural engineer recognise this as the detail requested, with its key elements? Score 1 if a key element is missing or wrong (e.g. a required connector/anchor not shown or not legible, wrong assembly), if hardware or members do not read as what they claim (a wall that looks like an empty box, a member that is a void), or if the layout is confusing. Score 0 if it is the wrong detail.
r2 MEMBERS SIZED AND POSITIONED, NO OVERLAPS OR GAPS. Score 1 for a concrete geometry defect: a member visibly clipped, short, or poking through another layer; a gap or overlap between members that should bear on each other; embedment or cover that contradicts the notes; roof/sheathing cut raggedly by the crop; dimension text that is struck through by a line or unreadable; leaders crossing EACH OTHER or running through note/dimension TEXT. A leader passing over hatch, over a dimension's extension line, or over a member outline is normal drafting and is NOT a defect; dimension text sitting over soil hatch is a defect only if it is hard to read. Estimate geometry from the picture and the engine summary; do not report sub-1/8-inch clashes you cannot verify. Otherwise 2.
r3 NOTE GRAMMAR AND CONSISTENCY. Office grammar: UPPERCASE, no trailing period, SIZE/QTY MATERIAL ITEM W/ ATTACHMENT @ SPACING QUALIFIER, standard abbreviations (W/ O.C. EA. CONT. TYP. PT MIN. MAX. CLR. DIA. GA. SIM. BOTT. T&B CONC. CMU EMBED. MFR. PER U.N.O. FTG. GRD. OSB PLY. HDR. DBL. STL. GALV. VERT. HORIZ.). Commas joining a spec to its qualifier ("..., INSTALL PER MFR.") are normal and are NOT a defect. Score 1 only for: lowercase or trailing periods, full sentences/commentary, non-standard abbreviations or invented ones, mixed number formats within the drawing, notes that contradict the drawing or each other (e.g. a note says 15/32 but the summary says 1/2), or a note that describes something not drawn. Otherwise 2.
r4 CITATIONS PLAUSIBLE, NONE INVENTED, UNCERTAINTY SURFACED. Use the NOTES AND CITATIONS list (the PNG only shows an asterisk). 2 = the citations that exist are real, apt code sections and the report tells the designer what to verify (a detail with a handful of apt citations is fine; do not demand a citation on every note). 1 = no citations at all where several are clearly warranted, a questionable or mismatched section, or the report claims a citation that the list does not show. 0 = invented or clearly wrong sections, or hardware/design presented as engineered or approved.`;

function runClaude(prompt, cwd, model) {
  const args = [
    "-p", prompt, "--output-format", "json", "--system-prompt", JUDGE_SYSTEM,
    "--tools", "Read", "--allowedTools", "Read", "--permission-mode", "acceptEdits",
    "--disable-slash-commands", "--strict-mcp-config", "--no-session-persistence",
    ...(model ? ["--model", model] : []),
  ];
  const r = spawnSync("claude", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], maxBuffer: 32 << 20, timeout: 300000 });
  if (r.status !== 0 && !r.stdout) throw new Error(`claude failed: ${r.stderr?.slice(0, 300)}`);
  return JSON.parse(r.stdout);
}

function extractJson(text) {
  const m = text.match(/\{[\s\S]*\}/);
  if (!m) throw new Error("no JSON in judge reply");
  return JSON.parse(m[0]);
}

function noteList(doc) {
  const out = [];
  for (const v of doc.views ?? []) for (const a of v.annotations ?? []) {
    if (a.type !== "note") continue;
    const cites = [].concat(a.cite ?? []).map((c) => `${c.code} ${c.section}${c.status ? ` [${c.status}]` : ""}`);
    out.push(`- ${a.text}${cites.length ? `  (cites: ${cites.join(", ")})` : ""}`);
  }
  return out.join("\n");
}

export function buildPrompt({ c, doc, summary, finalMessage, hasReference }) {
  return `REQUEST GIVEN TO THE DRAWING AGENT:
${c.prompt}${c.start ? `\n(The agent started from an existing detail and was asked to edit it; judge the edited result and whether only the requested change was made.)` : ""}

Read the drawing image ./detail.png now (use the Read tool).${hasReference ? "\nThe request attached a reference screenshot: also Read ./reference.png and judge how faithfully the drawing recreates it (geometry, notes, layout)." : ""}

ENGINE SUMMARY OF THE FINAL DOCUMENT (component, type, extents in feet-inches; x right, y up):
${summary}

NOTES AND CITATIONS IN THE DOCUMENT (cites render as "(CODE SECTION)*" on the sheet):
${noteList(doc) || "(none)"}

THE AGENT'S FINAL REPORT TO THE DESIGNER:
${(finalMessage ?? "").slice(0, 1800)}

${RUBRIC}

Reply with exactly this JSON shape (one short sentence per "why", naming the concrete defect or the reason no defect was found):
{"r1":{"score":0,"why":""},"r2":{"score":0,"why":""},"r3":{"score":0,"why":""},"r4":{"score":0,"why":""}}`;
}

export function judgeCase({ c, doc, summary, finalMessage, png, referencePng, model }) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "kerf-judge-"));
  try {
    fs.copyFileSync(png, path.join(dir, "detail.png"));
    if (referencePng) fs.copyFileSync(referencePng, path.join(dir, "reference.png"));
    const out = runClaude(buildPrompt({ c, doc, summary, finalMessage, hasReference: !!referencePng }), dir, model);
    const j = extractJson(String(out.result ?? ""));
    const scores = {};
    for (const k of CRITERIA) scores[k] = { score: Math.max(0, Math.min(2, Math.round(Number(j[k]?.score)))), why: String(j[k]?.why ?? "") };
    return { scores, total: CRITERIA.reduce((s, k) => s + scores[k].score, 0), cost_usd: out.total_cost_usd ?? null, turns: out.num_turns ?? null, model: out.modelUsage ? Object.keys(out.modelUsage).join(",") : null };
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

// ---- agreement statistics -----------------------------------------------------------------
export function agreement(rows) {
  // rows: [{id, judge: {r1..r4}, hand: {r1..r4}}]
  const n = rows.length;
  const per = {};
  for (const k of CRITERIA) {
    const d = rows.map((r) => r.judge[k] - r.hand[k]);
    per[k] = {
      mean_abs_diff: +(d.reduce((s, x) => s + Math.abs(x), 0) / n).toFixed(2),
      exact: +(d.filter((x) => x === 0).length / n).toFixed(2),
      bias: +(d.reduce((s, x) => s + x, 0) / n).toFixed(2),
    };
  }
  const jt = rows.map((r) => CRITERIA.reduce((s, k) => s + r.judge[k], 0));
  const ht = rows.map((r) => CRITERIA.reduce((s, k) => s + r.hand[k], 0));
  const mean = (a) => a.reduce((s, x) => s + x, 0) / a.length;
  const mj = mean(jt), mh = mean(ht);
  const cov = mean(jt.map((x, i) => (x - mj) * (ht[i] - mh)));
  const sj = Math.sqrt(mean(jt.map((x) => (x - mj) ** 2))), sh = Math.sqrt(mean(ht.map((x) => (x - mh) ** 2)));
  return {
    n,
    per_criterion: per,
    mean_abs_diff_all: +(CRITERIA.reduce((s, k) => s + per[k].mean_abs_diff, 0) / CRITERIA.length).toFixed(2),
    total_mean_abs_diff: +mean(jt.map((x, i) => Math.abs(x - ht[i]))).toFixed(2),
    total_bias: +(mj - mh).toFixed(2),
    total_pearson: sj && sh ? +(cov / (sj * sh)).toFixed(2) : null,
    within_one_total: +(jt.filter((x, i) => Math.abs(x - ht[i]) <= 1).length / n).toFixed(2),
  };
}

// ---- CLI ----------------------------------------------------------------------------------
function main() {
  const args = Object.fromEntries(process.argv.slice(2).reduce((acc, a, i, all) => {
    if (a.startsWith("--")) acc.push([a.slice(2), all[i + 1] === undefined || all[i + 1].startsWith("--") ? true : all[i + 1]]);
    return acc;
  }, []));
  if (!args.run) { console.error("usage: judge.mjs --run <runDir> [--only e01,e05] [--kerf bin] [--hand file --hand-key key] [--force]"); process.exit(2); }
  const runDir = path.resolve(String(args.run));
  const kerfBin = path.resolve(String(args.kerf ?? path.join(os.homedir(), "kerf-eval/bin/kerf-alpha4")));
  const only = args.only ? new Set(String(args.only).split(",")) : null;
  const model = args.model && args.model !== true ? String(args.model) : null;
  const style = JSON.parse(fs.readFileSync(path.join(root, "spec/styles/kerf-standard.kerfstyle.json"), "utf8"));
  const prompts = fs.readFileSync(path.join(here, "prompts.jsonl"), "utf8").trim().split("\n").map((l) => JSON.parse(l));
  const results = [];
  for (const c of prompts) {
    if (only && !only.has(c.id.split("-")[0]) && !only.has(c.id)) continue;
    const dir = path.join(runDir, c.id);
    const png = path.join(dir, `${c.id}-A.png`);
    const finalPath = path.join(dir, "final.kerf.json");
    if (!fs.existsSync(png) || !fs.existsSync(finalPath)) { console.log(`${c.id}: no PNG/doc, skipped`); continue; }
    const out = path.join(dir, "judge.json");
    let r;
    if (fs.existsSync(out) && !args.force) r = JSON.parse(fs.readFileSync(out, "utf8"));
    else {
      const doc = JSON.parse(fs.readFileSync(finalPath, "utf8"));
      const summary = JSON.parse(execFileSync(kerfBin, ["call", "check"], { input: JSON.stringify({ doc, style }), maxBuffer: 64 << 20 }).toString()).summary ?? "";
      const score = fs.existsSync(path.join(dir, "score.json")) ? JSON.parse(fs.readFileSync(path.join(dir, "score.json"), "utf8")) : {};
      process.stdout.write(`${c.id} ... `);
      r = judgeCase({ c, doc, summary, finalMessage: score.final_message, png, referencePng: c.attach ? path.join(root, c.attach) : null, model });
      fs.writeFileSync(out, JSON.stringify(r, null, 2));
    }
    console.log(`${CRITERIA.map((k) => r.scores[k].score).join(" ")}  total ${r.total}  $${r.cost_usd?.toFixed?.(3) ?? "?"}`);
    results.push({ id: c.id, ...r });
  }
  fs.writeFileSync(path.join(runDir, "judge-summary.json"), JSON.stringify({ judged: results.length, cost_usd: results.reduce((s, r) => s + (r.cost_usd ?? 0), 0), results }, null, 2));
  console.log(`judge cost $${results.reduce((s, r) => s + (r.cost_usd ?? 0), 0).toFixed(2)} for ${results.length} cases`);
  if (args.hand) {
    const hand = JSON.parse(fs.readFileSync(path.resolve(String(args.hand)), "utf8"))[String(args["hand-key"])];
    const rows = results.filter((r) => hand[r.id]).map((r) => ({ id: r.id, judge: Object.fromEntries(CRITERIA.map((k) => [k, r.scores[k].score])), hand: hand[r.id] }));
    const a = agreement(rows);
    console.log(JSON.stringify(a, null, 2));
    for (const r of rows) {
      const diffs = CRITERIA.filter((k) => r.judge[k] !== r.hand[k]).map((k) => `${k}:${r.judge[k]}vs${r.hand[k]}`);
      if (diffs.length) console.log(`  ${r.id} differs: ${diffs.join(" ")}`);
    }
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
