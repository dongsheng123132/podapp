import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { validatePackage } from "podapp-protocol/src/validate.mjs";
import {
  ACTION_SDK_FILE,
  generateActionSdk,
  sameGeneratedText,
} from "podapp-protocol/src/sdk.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const podsRoot = path.join(root, "pods");
const check = process.argv.includes("--check");
const json = process.argv.includes("--json");
const reports = [];

for (const entry of fs.readdirSync(podsRoot, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
  if (!entry.isDirectory()) continue;
  const dir = path.join(podsRoot, entry.name);
  if (!fs.existsSync(path.join(dir, "podapp.json"))) continue;

  const validation = validatePackage(dir);
  const blocking = validation.errors.filter((message) => !message.startsWith("Action SDK"));
  if (!validation.manifest || !validation.parity || blocking.length) {
    reports.push({ pod: entry.name, ok: false, status: "invalid", errors: blocking });
    continue;
  }

  const web = validation.manifest.package?.web;
  if (validation.manifest.package?.kind !== "web" || !web) {
    reports.push({ pod: entry.name, ok: true, status: "not-web", errors: [] });
    continue;
  }

  const webRoot = path.join(dir, web.root ?? "web");
  const sdkPath = path.join(webRoot, ACTION_SDK_FILE);
  const expected = generateActionSdk(validation.parity);
  const exists = fs.existsSync(sdkPath);
  const current = exists ? fs.readFileSync(sdkPath, "utf8") : "";
  const same = exists && sameGeneratedText(current, expected);
  if (!check && !same) {
    fs.mkdirSync(webRoot, { recursive: true });
    fs.writeFileSync(sdkPath, expected);
  }

  const errors = [];
  if (check && !same) errors.push(exists ? "generated SDK drifted" : "generated SDK missing");
  if (check && same) {
    const actionModule = path.join(webRoot, web.actions ?? "actions.mjs");
    try {
      await import(`${pathToFileURL(actionModule).href}?action-sdk-check=${Date.now()}`);
    } catch (error) {
      errors.push(`handler interface failed: ${error?.message ?? error}`);
    }
  }

  for (const filename of [web.actions ?? "actions.mjs", web.entry ?? "index.html"]) {
    const sourcePath = path.join(webRoot, filename);
    if (!fs.existsSync(sourcePath)) continue;
    const source = fs.readFileSync(sourcePath, "utf8");
    const literals = [...source.matchAll(/(["'])app\.[a-z0-9_.-]+\1/g)].map((match) => match[0]);
    if (literals.length) {
      errors.push(`${filename} repeats Action ID literals: ${[...new Set(literals)].join(", ")}`);
    }
  }

  reports.push({
    pod: entry.name,
    ok: errors.length === 0,
    status: same ? "current" : check ? "drifted" : "generated",
    actions: validation.parity.actions.length,
    path: path.relative(root, sdkPath).replaceAll("\\", "/"),
    errors,
  });
}

const ok = reports.every((report) => report.ok);
if (json) {
  process.stdout.write(`${JSON.stringify({ ok, data: { pods: reports } })}\n`);
} else {
  for (const report of reports) {
    process.stdout.write(`${report.ok ? "ok" : "failed"}\t${report.pod}\t${report.status}\t${report.actions ?? 0} action(s)\n`);
    for (const error of report.errors) process.stderr.write(`${report.pod}: ${error}\n`);
  }
}
process.exitCode = ok ? 0 : 1;
