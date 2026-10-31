// Coverage checker for the rbxoc API catalog.
//
// Verifies the agent-facing contract that the catalog is consistent:
//
//   1. Every catalog entry with implemented:true has a CLI subcommand.
//   2. Every implemented entry has a stable ID and a usable path.
//   3. Unimplemented entries say why.
//   4. CLI endpoint commands and catalog entries match in both directions.
//   5. Mutation classifications match the command contract.
//
// This is a source-level check. It does not call the Open Cloud API.

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

const root = path.resolve(import.meta.dirname, "..");
const catalogPath = path.join(root, "src", "catalog.rs");

function fail(message) {
	console.error(`FAIL  ${message}`);
	process.exitCode = 1;
}

function ok(message) {
	console.log(`ok    ${message}`);
}

function readCli(binary, args, json = false) {
	const result = spawnSync(binary, args, { encoding: "utf8" });
	if (result.error || result.status !== 0) {
		throw new Error(`${args.join(" ")}: ${result.error?.message ?? result.stderr ?? result.status}`);
	}
	return json ? JSON.parse(result.stdout) : result.stdout;
}

function commandLeaves(binary, prefix = []) {
	const help = readCli(binary, [...prefix, "--help"]).replaceAll("\r", "");
	const section = help.split("Commands:\n")[1]?.split("\n\n")[0];
	if (!section) return [prefix.join(" ")];
	const names = section.split("\n")
		.map((line) => line.match(/^  ([a-z][a-z0-9-]*)(?:\s|$)/)?.[1])
		.filter((name) => name && name !== "help");
	return names.flatMap((name) => commandLeaves(binary, [...prefix, name]));
}

function checkCommandMapping(binary, entries) {
	const commands = commandLeaves(binary).filter((command) =>
		!command.startsWith("api ") && !command.startsWith("assets ") && command !== "analytics catalog");
	const covered = new Set();
	const readActions = new Set(["check", "get", "list", "status", "history", "logs", "operation",
		"versions", "restart-forecast", "metrics", "dimensions", "draft", "revisions", "incidents", "image-id"]);
	for (const command of commands) {
		const id = command === "universe publish-message" ? "messaging.publish"
			: command.replace("analytics alerts ", "analytics alert ").replaceAll(" ", ".");
		const endpoint = entries.find((entry) => entry.id === id);
		if (!endpoint?.implemented) {
			fail(`CLI command ${command} lacks an implemented catalog entry (${id})`);
			continue;
		}
		covered.add(id);
		if (endpoint.mutating === readActions.has(command.split(" ").at(-1))) {
			fail(`wrong mutation classification for ${command} (${id})`);
		}
	}
	for (const entry of entries.filter((entry) => entry.implemented && entry.method !== "LOCAL")) {
		if (!covered.has(entry.id)) fail(`implemented catalog entry ${entry.id} lacks a CLI command`);
	}
	const mutations = readCli(binary, ["api", "mutating"], true);
	const expected = entries.filter((entry) => entry.implemented && entry.mutating).map((entry) => entry.id).sort();
	const actual = (mutations.data ?? []).map((entry) => entry.id).sort();
	if (!mutations.ok || JSON.stringify(expected) !== JSON.stringify(actual)) {
		fail("api mutating does not expose every implemented mutation");
	}
	if (!process.exitCode) ok(`${commands.length} CLI endpoint commands match the catalog in both directions, including mutations`);
}

// --- Read the catalog from source ---------------------------------------

const source = fs.readFileSync(catalogPath, "utf8");

// Each entry is `Endpoint { id: "...", ... implemented: true/false ... }`.
const entries = [];
const entryPattern = /Endpoint\s*\{([\s\S]*?)\n\s*\},/g;
for (const match of source.matchAll(entryPattern)) {
	const body = match[1];
	const id = body.match(/id:\s*"([^"]+)"/)?.[1];
	if (!id) continue;
	const method = body.match(/method:\s*"([^"]+)"/)?.[1] ?? "";
	const p = body.match(/path:\s*"([^"]+)"/)?.[1] ?? "";
	const implemented = /implemented:\s*true/.test(body);
	const summary = body.match(/summary:\s*"([^"]*)"/)?.[1] ?? "";
	const note = body.match(/note:\s*"([^"]*)"/)?.[1] ?? "";
	// auth: &[ApiKey, OAuth] - capture the names inside the brackets.
	const authRaw = body.match(/auth:\s*&\[([^\]]*)\]/)?.[1] ?? "";
	const auth = authRaw
		.split(",")
		.map((x) => x.trim())
		.filter(Boolean);
	entries.push({ id, method, path: p, implemented, summary, note, auth });
}

if (entries.length === 0) {
	fail("parsed no catalog entries; the regex may be out of date");
	process.exit(1);
}

// --- Invariants ----------------------------------------------------------

// 1. Unique IDs.
const ids = entries.map((e) => e.id);
const dupes = ids.filter((id, i) => ids.indexOf(id) !== i);
if (dupes.length > 0) fail(`duplicate IDs: ${[...new Set(dupes)].join(", ")}`);
else ok(`${ids.length} unique catalog IDs`);

// 2. Implemented entries need a path and a summary.
const broken = entries.filter((e) => e.implemented && (!e.path || !e.summary));
if (broken.length > 0) fail(`implemented entries missing path/summary: ${broken.map((e) => e.id).join(", ")}`);
else ok("every implemented entry has a path and summary");

// 3. HTTP entries start with "/"; local entries do not have to.
const badPath = entries.filter((e) => e.implemented && e.method !== "LOCAL" && !e.path.startsWith("/"));
if (badPath.length > 0) fail(`non-local paths must start with /: ${badPath.map((e) => e.id).join(", ")}`);
else ok("every HTTP path starts with /");

// 4. Unimplemented entries explain themselves.
const unexplained = entries.filter((e) => !e.implemented && !e.note);
if (unexplained.length > 0) fail(`unimplemented entries without a note: ${unexplained.map((e) => e.id).join(", ")}`);
else ok("every unimplemented entry states why");

// 5. Legacy cookie entries must not be implemented. Parse the auth list rather
//    than slicing source, so a neighbouring entry cannot cause a false hit.
const cookieImplemented = entries.filter(
	(e) => e.implemented && e.auth.length === 1 && e.auth[0] === "LegacyCookie",
);
if (cookieImplemented.length > 0) {
	fail(`cookie-only entries must not be implemented: ${cookieImplemented.map((e) => e.id).join(", ")}`);
} else {
	ok("no cookie-only entry is implemented");
}

// --- Cross-check against the CLI ----------------------------------------

const target = process.env.CARGO_TARGET_DIR
	? path.resolve(root, process.env.CARGO_TARGET_DIR)
	: path.join(root, "target");
const binary = path.join(target, "release", process.platform === "win32" ? "rbxoc.exe" : "rbxoc");
if (!fs.existsSync(binary)) {
	fail(`release binary missing at ${binary}; run \`mise run build\` first`);
} else {
	const result = spawnSync(binary, ["api", "list"], { encoding: "utf8" });
	if (result.error) {
		fail(`cannot run the binary: ${result.error.message}`);
	} else {
		let parsed;
		try {
			parsed = JSON.parse(result.stdout);
		} catch (e) {
			fail(`api list did not return JSON: ${e.message}`);
		}
		if (parsed) {
			if (!parsed.ok) fail("api list reported ok:false");
			const fromCli = parsed.data ?? [];
			if (fromCli.length !== entries.length) {
				fail(`catalog/source mismatch: source ${entries.length}, CLI ${fromCli.length}`);
			} else {
				ok(`CLI reports the same ${fromCli.length} entries as the source`);
			}
			const impl = fromCli.filter((e) => e.implemented).length;
			console.log(`\ncatalog: ${fromCli.length} entries, ${impl} implemented`);
			const byDomain = {};
			for (const e of fromCli) {
				if (!e.implemented) continue;
				const domain = e.id.split(".")[0];
				byDomain[domain] = (byDomain[domain] ?? 0) + 1;
			}
			for (const [domain, count] of Object.entries(byDomain).sort()) {
				console.log(`  ${domain.padEnd(20)} ${count}`);
			}
			try {
				checkCommandMapping(binary, fromCli);
			} catch (error) {
				fail(`command/catalog check: ${error.message}`);
			}
		}
	}
}

if (process.exitCode) {
	console.error("\ncoverage check FAILED");
} else {
	console.log("\ncoverage check passed");
}
