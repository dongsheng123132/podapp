import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const podsRoot = path.join(root, "pods");
const cli = path.join(root, "node_modules", "action-parity", "bin", "action-parity.mjs");
const json = process.argv.includes("--json");

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function checkEvidenceRefs(manifestPath) {
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const refs = [...new Set(manifest.actions
    .map((action) => action.execution?.headless_evidence)
    .filter(Boolean))];
  const errors = [];
  for (const ref of refs) {
    const match = /^cargo:test:([^/]+)\/([^:]+)::([A-Za-z0-9_]+)$/.exec(ref);
    if (!match) continue;
    const [, crate, target, testName] = match;
    const sourcePath = path.join(root, "crates", crate, "tests", `${target}.rs`);
    if (!fs.existsSync(sourcePath)) {
      errors.push(`${ref}: test target does not exist`);
      continue;
    }
    const source = fs.readFileSync(sourcePath, "utf8");
    const testPattern = new RegExp(
      `#\\s*\\[\\s*(?:(?:tokio|async_std)::)?test[^\\]]*\\]\\s*(?:async\\s+)?fn\\s+${escapeRegExp(testName)}\\s*\\(`,
    );
    if (!testPattern.test(source)) errors.push(`${ref}: test function does not exist`);
  }
  return { refs, errors };
}

if (!fs.existsSync(cli)) {
  process.stderr.write("ActionParity CLI 未安装；先运行 npm ci。\n");
  process.exit(1);
}

const reports = [];
for (const entry of fs.readdirSync(podsRoot, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
  if (!entry.isDirectory()) continue;
  const manifest = path.join(podsRoot, entry.name, "action-parity.json");
  if (!fs.existsSync(manifest)) continue;

  const run = spawnSync(process.execPath, [cli, "validate", manifest, "--json", "--quiet"], {
    cwd: root,
    encoding: "utf8",
    timeout: 15_000,
    maxBuffer: 2 * 1024 * 1024,
  });
  let result;
  try {
    result = JSON.parse(run.stdout.trim());
  } catch {
    result = null;
  }
  let evidence = { refs: [], errors: [] };
  try {
    evidence = checkEvidenceRefs(manifest);
  } catch (error) {
    evidence.errors.push(`cannot inspect evidence refs: ${error?.message ?? error}`);
  }
  const ok = run.status === 0 && result?.ok === true && result?.data?.ok === true && evidence.errors.length === 0;
  reports.push({
    pod: entry.name,
    ok,
    actions: result?.data?.summary?.actions ?? 0,
    evidence: result?.data?.evidence?.status ?? "unknown",
    errors: result?.data?.summary?.errors ?? (ok ? 0 : 1),
    warnings: result?.data?.summary?.warnings ?? 0,
    evidence_refs: evidence.refs.length,
    message: ok
      ? undefined
      : evidence.errors.join("; ") || result?.error?.message || run.stderr.trim() || "validation failed",
  });
}

const ok = reports.length > 0 && reports.every((report) => report.ok);
if (json) {
  process.stdout.write(`${JSON.stringify({ ok, data: { manifests: reports } })}\n`);
} else {
  for (const report of reports) {
    process.stdout.write(
      `${report.ok ? "ok" : "failed"}\t${report.pod}\t${report.actions} action(s)\t${report.evidence_refs} evidence ref(s)\t${report.evidence}\n`,
    );
    if (report.message) process.stderr.write(`${report.pod}: ${report.message}\n`);
  }
}
process.exitCode = ok ? 0 : 1;
