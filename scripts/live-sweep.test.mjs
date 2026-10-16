import assert from "node:assert/strict";
import { test } from "node:test";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { existsSync } from "node:fs";
import { buildPlan, cases } from "./live-sweep-cases.mjs";
import { runSweep } from "./live-sweep-runner.mjs";

function fixtureEnv() {
  const env = {};
  for (const c of cases) for (const a of c.args) if (typeof a !== "string") env[a.env] = "1";
  Object.assign(env, {
    DATASTORE_NAME: "fixture-store", DATASTORE_SCOPE: "fixture-scope", DATASTORE_ENTRY: "fixture-entry",
    CONFIG_REPOSITORY: "fixture-repository", MESSAGE_TOPIC: "fixture-topic", JOB_ID: "fixture-job",
    PLACE_FILE: "fixture.rbxlx", THUMBNAIL_FILE: "fixture.png", MESSAGE_JSON: "{}", DATASTORE_VALUE: "{}",
    ANALYTICS_QUERY: '{"metrics":[],"startTime":"2026-01-01T00:00:00Z","endTime":"2026-01-02T00:00:00Z"}',
  });
  return env;
}

function execute(env, spawn) {
  const logs = [];
  const status = runSweep({ env, spawn, log: (s) => logs.push(s), error: (s) => logs.push(s) });
  return { status, logs };
}

test("default and non-exact opt-ins spawn only read-only cases", () => {
  for (const opt of [undefined, "0", "true", "yes", " 1"]) {
    const env = { ...fixtureEnv(), RBXOC_SWEEP_MUTATE: opt };
    const plan = buildPlan(env);
    assert(plan.cases.every((c) => !c.mutating));
    let calls = 0;
    const out = execute(env, (_, args) => {
      assert(plan.cases.some((c) => JSON.stringify(c.args) === JSON.stringify(args) && !c.mutating));
      calls++;
      return { status: 0, stdout: '{"ok":true}' };
    });
    assert.equal(out.status, 0);
    assert.equal(calls, plan.cases.length);
  }
});

test("mutation opt-in prints all targets before the first spawn", () => {
  const env = { ...fixtureEnv(), RBXOC_SWEEP_MUTATE: "1" };
  const logs = [];
  let calls = 0;
  const status = runSweep({ env, log: (s) => logs.push(s), error: assert.fail, spawn: () => {
    assert(logs.some((s) => s.includes("MUTATIONS EXPLICITLY ENABLED")));
    assert(logs.some((s) => s.includes('"UNIVERSE_ID":"1"') && s.includes('"PLACE_ID":"1"')));
    calls++;
    return { status: 0, stdout: '{"ok":true}' };
  } });
  assert.equal(status, 0);
  assert.equal(calls, cases.length);
});

test("missing/invalid targets or unauthorized selected mutations cause zero spawns", () => {
  const base = fixtureEnv();
  for (const env of [
    {}, { ...base, ASSET_ID: "" }, { ...base, PLACE_ID: "not-an-id" },
    { ...base, RBXOC_SWEEP_CASES: "thumbnail upload" },
    { ...base, RBXOC_SWEEP_MUTATE: "1", GAMEPASS_ID: undefined },
    { ...base, RBXOC_SWEEP_CASES: "not-a-case" },
  ]) {
    const out = execute(env, () => assert.fail("spawn before complete preflight"));
    assert.equal(out.status, 2);
  }
});

test("selected read only cases need only their own explicit targets", () => {
  const plan = buildPlan({ RBXOC_SWEEP_CASES: "universe get", UNIVERSE_ID: "1" });
  assert.equal(plan.cases.length, 1);
  assert.equal(plan.cases[0].label, "universe get");
});

test("spawn, JSON and runtime failures are not reported as successful sweeps", () => {
  const env = { UNIVERSE_ID: "1", RBXOC_SWEEP_CASES: "universe get" };
  for (const result of [
    { status: 1, stdout: '{"ok":false}' }, { status: 1, stdout: '{"ok":true}' },
    { status: 0, stdout: "not JSON" }, { status: null, error: new Error("fixture"), stderr: "SECRET" },
  ]) {
    const out = execute(env, () => result);
    assert.equal(out.status, 1);
    assert(!out.logs.join("\n").includes("SECRET"));
  }
});

const suffix = process.platform === "win32" ? ".exe" : "";
const release = fileURLToPath(new URL(`../target/release/rbxoc${suffix}`, import.meta.url));
const debug = fileURLToPath(new URL(`../target/debug/rbxoc${suffix}`, import.meta.url));
const bin = process.env.RBXOC_TEST_BIN || (existsSync(release) ? release : debug);

test("shared cases match the built catalog's mutation policy and current CLI", () => {
  const out = spawnSync(bin, ["--format", "json", "api", "list"], { encoding: "utf8" });
  assert.equal(out.status, 0, out.stderr || out.error?.message);
  const catalog = new Map(JSON.parse(out.stdout).data.map((e) => [e.id, e]));
  const plan = buildPlan({ ...fixtureEnv(), RBXOC_SWEEP_MUTATE: "1" });
  for (const c of plan.cases) {
    if (c.endpoint) {
      assert(catalog.has(c.endpoint), c.endpoint);
      assert.equal(catalog.get(c.endpoint).mutating, c.mutating, c.label);
      assert.equal(catalog.get(c.endpoint).implemented, true, c.label);
    }
    // --help never performs a cloud request; stale subcommands/flags still fail.
    const help = spawnSync(bin, [...c.args, "--help"], { encoding: "utf8" });
    assert.equal(help.status, 0, `${c.label}: ${help.stdout} ${help.stderr}`);
  }
  assert(plan.cases.find((c) => c.label === "thumbnail upload").args.includes("--files"));
  assert(plan.cases.find((c) => c.label === "analytics metrics").args.includes("--query"));
});

test("both entry points fail closed without targets", () => {
  for (const file of ["live-sweep.mjs", "live-sweep-ext.mjs"]) {
    const result = spawnSync(process.execPath, [fileURLToPath(new URL(file, import.meta.url))], { env: {}, encoding: "utf8" });
    assert.equal(result.status, 2);
    assert(result.stderr.includes("required env UNIVERSE_ID"));
  }
});
