import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { buildPlan } from "./live-sweep-cases.mjs";

/** Dependency injection lets tests prove safety without any real API calls. */
export function runSweep({ env = process.env, spawn = spawnSync, log = console.log, error = console.error } = {}) {
  let plan;
  try {
    plan = buildPlan(env);
  } catch (err) {
    error(`Sweep configuration rejected: ${err.message}`);
    return 2;
  }
  const executable = env.RBXOC || fileURLToPath(new URL(`../target/release/rbxoc${process.platform === "win32" ? ".exe" : ""}`, import.meta.url));
  log(`rbxoc: ${executable}`);
  log(`mode=${plan.mutating ? "MUTATIONS EXPLICITLY ENABLED" : "READ ONLY (mutations skipped)"}`);
  log(`targets=${JSON.stringify(plan.targets)}`);
  log("=== live sweep: target review above precedes all commands ===");
  let failures = 0;
  for (const c of plan.cases) {
    const result = spawn(executable, c.args, { env, encoding: "utf8", maxBuffer: 10 * 1024 * 1024, timeout: 120_000 });
    let summary;
    try {
      const data = JSON.parse(result.stdout || "");
      const ok = !result.error && result.status === 0 && data.ok === true;
      if (!ok) failures++;
      summary = `${ok ? "OK" : "ERR"} ${String(data.summary ?? "").slice(0, 120)}`;
    } catch {
      failures++;
      // Do not dump raw stderr/environment: they can contain consumer secrets.
      summary = `ERR exit=${result.status ?? "none"} signal=${result.signal ?? "none"}; no JSON envelope`;
    }
    log(`${c.label.padEnd(32)} ${summary}`);
  }
  log(`=== ${plan.cases.length - failures} ok / ${failures} err / ${plan.cases.length} total ===`);
  return failures ? 1 : 0;
}
