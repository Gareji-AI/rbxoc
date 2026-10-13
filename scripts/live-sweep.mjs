#!/usr/bin/env node
/**
 * Live Open Cloud sweep for rbxoc. Run from roblox-ts-ai-native-template so .env loads:
 *   cd ../roblox-ts-ai-native-template && mise exec -- node ../rbxoc/scripts/live-sweep.mjs
 */
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const RBXOC =
	process.env.RBXOC ??
	path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "target", "release", "rbxoc.exe");

const U = process.env.UNIVERSE_ID ?? "10770536071";
const P = process.env.PLACE_ID ?? "104295866864681";
const ASSET = process.env.ASSET_ID ?? "119311068431724";
const GAMEPASS = process.env.GAMEPASS_ID ?? "2029442313";

const cases = [
	["auth check", ["auth", "check", "--universe-id", U]],
	["universe get", ["universe", "get", "--universe-id", U]],
	["place get", ["place", "get", "--universe-id", U, "--place-id", P]],
	["place history", ["place", "history", "--place-id", P]],
	["thumbnail list", ["thumbnail", "list", "--universe-id", U]],
	["gamepass list", ["gamepass", "list", "--universe-id", U]],
	["devproduct list", ["devproduct", "list", "--universe-id", U]],
	["event list", ["event", "list", "--universe-id", U]],
	["server list", ["server", "list", "--universe-id", U, "--place-id", P, "--version", "1"]],
	["config get", ["config", "get", "--universe-id", U, "--repository", "game"]],
	["asset get", ["asset", "get", "--asset-id", ASSET]],
	["asset versions", ["asset", "versions", "--asset-id", ASSET]],
	[
		"ordered list",
		["ordered-datastore", "list", "--universe-id", U, "--store", "rbxoc-e2e", "--scope", "global"],
	],
	[
		"ordered set",
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
			"probe",
			"--value",
			"100",
		],
	],
	[
		"ordered get",
		[
			"ordered-datastore",
			"get",
			"--universe-id",
			U,
			"--store",
			"rbxoc-e2e",
			"--scope",
			"global",
			"--entry",
			"probe",
		],
	],
	[
		"ordered increment",
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
			"probe",
			"--amount",
			"1",
		],
	],
	[
		"datastore set (create)",
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
		"datastore list",
		["datastore", "list", "--universe-id", U, "--store", "rbxoc-e2e"],
	],
	[
		"universe publish-message",
		["universe", "publish-message", "--universe-id", U, "--topic", "rbxoc-e2e", "--message", '{"ping":1}'],
	],
	[
		"permissions grant",
		[
			"permissions",
			"grant",
			"--asset-id",
			ASSET,
			"--subject-type",
			"group",
			"--subject-id",
			"333424621",
			"--action",
			"use",
		],
	],
	[
		"place publish Saved",
		["place", "publish", "--universe-id", U, "--place-id", P, "--file", "places/game.rbxlx"],
	],
	[
		"thumbnail upload dry",
		["thumbnail", "upload", "--universe-id", U, "--file", "probe.png"],
	],
	[
		"analytics query",
		[
			"analytics",
			"query",
			"--universe-id",
			U,
			"--metric",
			"DailyActiveUsers",
			"--granularity",
			"day",
			"--start",
			"2026-10-01",
			"--end",
			"2026-10-12",
		],
	],
	["analytics alerts list", ["analytics", "alerts", "list", "--universe-id", U]],
];

function run(label, args) {
	process.stdout.write(`${label.padEnd(36)} `);
	const r = spawnSync(RBXOC, args, {
		encoding: "utf8",
		maxBuffer: 10 * 1024 * 1024,
		timeout: 120_000,
	});
	const text = (r.stdout || "") + (r.stderr || "");
	let summary = "PARSE-FAIL";
	try {
		const d = JSON.parse(text);
		summary = `${d.ok ? "OK" : "ERR"} ${String(d.summary ?? "").slice(0, 72)}`;
	} catch {
		summary = `EXIT ${r.status} ${text.slice(0, 60)}`;
	}
	console.log(summary);
	return { label, ok: summary.startsWith("OK"), summary };
}

console.log(`rbxoc: ${RBXOC}`);
console.log(`universe=${U} place=${P}`);
console.log("=== live sweep ===");
const results = cases.map(([label, args]) => run(label, args));
const ok = results.filter((x) => x.ok).length;
const err = results.length - ok;
console.log(`=== ${ok} ok / ${err} err / ${results.length} total ===`);
