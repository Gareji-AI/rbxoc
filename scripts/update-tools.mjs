import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

// プロジェクト限定のツール更新。
//
// mise の self_update.auto と tool_update.global_auto は global-only の設定で、
// リポジトリの mise.toml からは有効化できない（mise がセキュリティ理由で拒否する）。
// そのため、このリポジトリのツール更新はこのスクリプトで明示的に行う。
//
// 使い方:
//   node scripts/update-tools.mjs            # 最新版を調べて表示のみ
//   node scripts/update-tools.mjs --apply    # mise.toml を書き換え、install まで実行

const root = path.resolve(import.meta.dirname, "..");
const manifestPath = path.join(root, "mise.toml");
const apply = process.argv.includes("--apply");

// 更新対象。mise.toml の [tools] と対応させる。
const TRACKED = [
	{ key: "rust", label: "rust", bare: true },
	{ key: "github:rojo-rbx/rojo", label: "rojo" },
	{ key: "github:pesde-pkg/pesde", label: "pesde" },
];

function mise(args) {
	const result = spawnSync("mise", args, { cwd: root, encoding: "utf8", shell: false });
	if (result.error) {
		console.error(`mise を実行できません: ${result.error.message}`);
		process.exit(1);
	}
	return result;
}

// [tools] セクションだけを対象にする。他のテーブルの同名キーを拾わない。
function parseTools(text) {
	const lines = text.split("\n");
	const start = lines.findIndex((line) => line.trim() === "[tools]");
	if (start === -1) {
		console.error("mise.toml に [tools] セクションがありません。");
		process.exit(1);
	}
	const tools = new Map();
	for (let i = start + 1; i < lines.length; i++) {
		const line = lines[i];
		if (/^\s*\[/.test(line)) break;
		// 裸の代入 `rust = "1.99"` と引用キー `"github:..." = "x"` の両方。
		const match =
			line.match(/^\s*"([^"]+)"\s*=\s*"([^"]+)"/) ??
			line.match(/^\s*([A-Za-z0-9_-]+)\s*=\s*"([^"]+)"/);
		if (match) tools.set(match[1], { value: match[2], line: i });
	}
	return { tools, lines };
}

// pesde は "+registry.0.2.3" のような build メタデータ付きで配布される。
// mise.toml には plain な SemVer を書く。
function normalize(label, version) {
	if (label === "pesde") return version.split("+")[0];
	return version;
}

const { tools, lines } = parseTools(fs.readFileSync(manifestPath, "utf8"));
const updates = [];

for (const { key, label } of TRACKED) {
	const entry = tools.get(key);
	if (!entry) {
		console.warn(`未登録: ${key}（mise.toml の [tools] にありません）`);
		continue;
	}
	// `mise latest` は安定版を解決する。`ls-remote` は nightly / beta /
	// stable のような別名も返すため、単独では最新版の判定に使えない。
	const result = mise(["latest", key]);
	if (result.status !== 0) {
		console.warn(`取得失敗: ${key}`);
		continue;
	}
	const latest = result.stdout
		.split("\n")
		.map((line) => line.trim())
		.filter((line) => line && !line.startsWith("mise "))
		.at(-1);
	if (!latest) {
		console.warn(`候補なし: ${key}`);
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
	console.log("更新対象のツールがありません。");
	process.exit(0);
}

let changed = 0;
for (const update of updates) {
	if (update.current === update.target) {
		console.log(`最新  ${update.label} ${update.current}`);
		continue;
	}
	console.log(`更新  ${update.label} ${update.current} -> ${update.target}（配布版 ${update.latest}）`);
	changed++;
	if (apply) {
		lines[update.line] = lines[update.line].replace(
			`"${update.current}"`,
			`"${update.target}"`,
		);
	}
}

if (changed === 0) {
	console.log("すべて最新です。");
	process.exit(0);
}

if (!apply) {
	console.log("\n適用するには --apply を付けて再実行してください。");
	process.exit(0);
}

fs.writeFileSync(manifestPath, lines.join("\n"), "utf8");
console.log("\nmise.toml を更新しました。mise install を実行します。");

const install = mise(["install"]);
process.stdout.write(install.stdout ?? "");
process.stderr.write(install.stderr ?? "");
if (install.status !== 0) {
	console.error("mise install が失敗しました。mise.toml を確認してください。");
	process.exit(install.status ?? 1);
}

console.log("\nmise install 完了。`mise run check` で検証してください。");
