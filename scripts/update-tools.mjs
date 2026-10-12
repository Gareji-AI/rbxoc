import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

// Project-scoped tool updates.
//
// mise's self_update.auto and tool_update.global_auto are global-only settings.
// A repository mise.toml cannot enable them; mise rejects them for security
// reasons. Tool updates for this repository therefore run explicitly here.
//
// Usage:
//   node scripts/update-tools.mjs            # report the latest versions only
//   node scripts/update-tools.mjs --apply    # rewrite mise.toml and install

const root = path.resolve(import.meta.dirname, "..");
const manifestPath = path.join(root, "mise.toml");
const apply = process.argv.includes("--apply");

// Update targets. These mirror [tools] in mise.toml.
// Only Rust. rbxoc shells out to no Roblox tooling, so none is pinned.
const TRACKED = [{ key: "rust", label: "rust" }];

function mise(args) {
	const result = spawnSync("mise", args, { cwd: root, encoding: "utf8", shell: false });
	if (result.error) {
		console.error(`cannot run mise: ${result.error.message}`);
		process.exit(1);
	}
	return result;
}

// Parse [tools] only, so a same-named key in another table is not picked up.
function parseTools(text) {
	const lines = text.split("\n");
	const start = lines.findIndex((line) => line.trim() === "[tools]");
	if (start === -1) {
		console.error("mise.toml has no [tools] section.");
		process.exit(1);
	}
	const tools = new Map();
	for (let i = start + 1; i < lines.length; i++) {
		const line = lines[i];
		if (/^\s*\[/.test(line)) break;
		// Accept both a bare assignment (`rust = "1.99.0"`) and a quoted key.
		const match =
			line.match(/^\s*"([^"]+)"\s*=\s*"([^"]+)"/) ??
			line.match(/^\s*([A-Za-z0-9_-]+)\s*=\s*"([^"]+)"/);
		if (match) tools.set(match[1], { value: match[2], line: i });
	}
	return { tools, lines };
}

// pesde ships with build metadata such as "+registry.0.2.3". mise.toml keeps
// the plain SemVer.
function normalize(label, version) {
	if (label === "pesde") return version.split("+")[0];
	return version;
}

const { tools, lines } = parseTools(fs.readFileSync(manifestPath, "utf8"));
const updates = [];

for (const { key, label } of TRACKED) {
	const entry = tools.get(key);
	if (!entry) {
		console.warn(`not registered: ${key} (missing from [tools] in mise.toml)`);
		continue;
	}
	// `mise latest` resolves the stable release. `ls-remote` also returns
	// aliases such as nightly / beta / stable, so it cannot be used alone.
	const result = mise(["latest", key]);
	if (result.status !== 0) {
		console.warn(`lookup failed: ${key}`);
		continue;
	}
	const latest = result.stdout
		.split("\n")
		.map((line) => line.trim())
		.filter((line) => line && !line.startsWith("mise "))
		.at(-1);
	if (!latest) {
		console.warn(`no candidate: ${key}`);
		continue;
	}
	updates.push({
		key,
		label,
		current: entry.value,
		latest,
		target: normalize(label, latest),
		line: entry.line,
	});
}

if (updates.length === 0) {
	console.log("No tracked tools found.");
	process.exit(0);
}

let changed = 0;
for (const update of updates) {
	if (update.current === update.target) {
		console.log(`current  ${update.label} ${update.current}`);
		continue;
	}
	console.log(
		`update   ${update.label} ${update.current} -> ${update.target} (published ${update.latest})`,
	);
	changed++;
	if (apply) {
		lines[update.line] = lines[update.line].replace(
			`"${update.current}"`,
			`"${update.target}"`,
		);
	}
}

if (changed === 0) {
	console.log("Everything is current.");
	process.exit(0);
}

if (!apply) {
	console.log("\nRe-run with --apply to write the changes.");
	process.exit(0);
}

fs.writeFileSync(manifestPath, lines.join("\n"), "utf8");
console.log("\nUpdated mise.toml. Running mise install.");

const install = mise(["install"]);
process.stdout.write(install.stdout ?? "");
process.stderr.write(install.stderr ?? "");
if (install.status !== 0) {
	console.error("mise install failed. Check mise.toml.");
	process.exit(install.status ?? 1);
}

console.log("\nmise install finished. Verify with `mise run check`.");
