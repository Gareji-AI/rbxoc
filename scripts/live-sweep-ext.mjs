#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const RBXOC = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "target", "release", "rbxoc.exe");
const U = "10770536071";
const P = "104295866864681";
const ASSET = "119311068431724";
const GP = "2029442313";
const DP = "3718258826";

const cases = [
	["devproduct update", ["devproduct", "update", "--universe-id", U, "--product-id", DP, "--price", "2"]],
	["gamepass update", ["gamepass", "update", "--universe-id", U, "--game-pass-id", GP, "--price", "5"]],
	[
		"ordered increment (new entry)",
		[
			"ordered-datastore",
			"increment",
			"--universe-id",
			U,
			"--store",
			"rbxoc-e2e",
			"--scope",
			"global",
			"--entry",
			"probe-create",
			"--amount",
			"10",
		],
	],
	[
		"ordered set --create (new entry)",
		[
			"ordered-datastore",
			"set",
			"--universe-id",
			U,
			"--store",
			"rbxoc-e2e",
			"--scope",
			"global",
			"--entry",
			"probe-set-create",
			"--value",
			"7",
			"--create",
		],
	],
	[
		"ordered delete",
		[
			"ordered-datastore",
			"delete",
			"--universe-id",
			U,
			"--store",
			"rbxoc-e2e",
			"--scope",
			"global",
			"--entry",
			"probe-create",
		],
	],
	[
		"datastore set",
		[
			"datastore",
			"set",
			"--universe-id",
			U,
			"--store",
			"rbxoc-e2e",
			"--entry",
			"probe",
			"--value",
			'{"ok":true}',
			"--create",
		],
	],
	[
		"datastore get",
		["datastore", "get", "--universe-id", U, "--store", "rbxoc-e2e", "--entry", "probe"],
	],
	[
		"analytics metrics (api key)",
		[
			"analytics",
			"metrics",
			"--universe-id",
			U,
			"--query",
			'{"metrics":[{"metric":"DailyActiveUsers"}],"startTime":"2026-10-01T00:00:00Z","endTime":"2026-10-08T00:00:00Z","granularity":"Day"}',
		],
	],
	["analytics catalog", ["analytics", "catalog"]],
	["place publish", ["place", "publish", "--universe-id", U, "--place-id", P, "--file", "places/game.rbxlx"]],
	[
		"server logs (no servers)",
		["server", "logs", "--universe-id", U, "--place-id", P, "--version", "1", "--job-id", "00000000-0000-0000-0000-000000000000"],
	],
];

function run(label, args) {
	process.stdout.write(`${label.padEnd(38)} `);
	const r = spawnSync(RBXOC, args, { encoding: "utf8", timeout: 120_000 });
	let summary = "PARSE-FAIL";
	try {
		const d = JSON.parse(r.stdout || r.stderr);
		summary = `${d.ok ? "OK" : "ERR"} ${String(d.summary ?? "").slice(0, 68)}`;
	} catch {
		summary = `EXIT ${r.status}`;
	}
	console.log(summary);
}

for (const [label, args] of cases) run(label, args);
