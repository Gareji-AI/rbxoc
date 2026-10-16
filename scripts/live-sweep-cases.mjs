/** One reviewed case list for both live-sweep entry points. No production IDs. */
const arg = (name) => ({ env: name });
const universe = ["--universe-id", arg("UNIVERSE_ID")];
const place = [...universe, "--place-id", arg("PLACE_ID")];
const store = [...universe, "--store", arg("DATASTORE_NAME")];
const ordered = [...store, "--scope", arg("DATASTORE_SCOPE")];
const entry = ["--entry", arg("DATASTORE_ENTRY")];
const asset = ["--asset-id", arg("ASSET_ID")];
const version = ["--version", arg("SERVER_VERSION")];
const define = (label, endpoint, mutating, args) => ({ label, endpoint, mutating, args });

export const cases = [
  define("auth check", "auth.check", false, ["auth", "check", ...universe]),
  // universe get shares the auth probe's GET route; no separate catalog ID.
  define("universe get", null, false, ["universe", "get", ...universe]),
  define("place get", "place.get", false, ["place", "get", ...place]),
  define("place history", "place.history", false, ["place", "history", "--place-id", arg("PLACE_ID")]),
  define("thumbnail list", "thumbnail.list", false, ["thumbnail", "list", ...universe]),
  define("gamepass list", "gamepass.list", false, ["gamepass", "list", ...universe]),
  define("devproduct list", "devproduct.list", false, ["devproduct", "list", ...universe]),
  define("event list", "event.list", false, ["event", "list", ...universe]),
  define("server list", "server.list", false, ["server", "list", ...place, ...version]),
  define("server logs", "server.logs", false, ["server", "logs", ...place, ...version, "--job-id", arg("JOB_ID")]),
  define("config get", "config.get", false, ["config", "get", ...universe, "--repository", arg("CONFIG_REPOSITORY")]),
  define("asset get", "asset.get", false, ["asset", "get", ...asset]),
  define("asset versions", "asset.versions", false, ["asset", "versions", ...asset]),
  define("ordered list", "ordered-datastore.list", false, ["ordered-datastore", "list", ...ordered]),
  define("ordered get", "ordered-datastore.get", false, ["ordered-datastore", "get", ...ordered, ...entry]),
  define("datastore get", "datastore.get", false, ["datastore", "get", ...store, ...entry]),
  define("datastore list", "datastore.list", false, ["datastore", "list", ...store]),
  define("analytics metrics", "analytics.metrics", false, ["analytics", "metrics", ...universe, "--query", arg("ANALYTICS_QUERY")]),
  define("analytics catalog", null, false, ["analytics", "catalog"]),
  define("analytics alerts list", "analytics.alert.list", false, ["analytics", "alerts", "list", ...universe]),
  define("ordered set", "ordered-datastore.set", true, ["ordered-datastore", "set", ...ordered, ...entry, "--value", arg("ORDERED_VALUE"), "--create"]),
  define("ordered increment", "ordered-datastore.increment", true, ["ordered-datastore", "increment", ...ordered, ...entry, "--amount", arg("ORDERED_AMOUNT")]),
  define("ordered delete", "ordered-datastore.delete", true, ["ordered-datastore", "delete", ...ordered, ...entry]),
  define("datastore set", "datastore.set", true, ["datastore", "set", ...store, ...entry, "--value", arg("DATASTORE_VALUE"), "--create"]),
  define("universe publish-message", "messaging.publish", true, ["universe", "publish-message", ...universe, "--topic", arg("MESSAGE_TOPIC"), "--message", arg("MESSAGE_JSON")]),
  define("permissions grant", "permissions.grant", true, ["permissions", "grant", ...asset, "--subject-type", "group", "--subject-id", arg("GROUP_ID"), "--action", "use"]),
  define("place publish", "place.publish", true, ["place", "publish", ...place, "--file", arg("PLACE_FILE")]),
  define("thumbnail upload", "thumbnail.upload", true, ["thumbnail", "upload", ...universe, "--files", arg("THUMBNAIL_FILE")]),
  define("gamepass update", "gamepass.update", true, ["gamepass", "update", ...universe, "--game-pass-id", arg("GAMEPASS_ID"), "--price", arg("GAMEPASS_PRICE")]),
];

/** Resolve the entire plan before the first command; missing config is fail-closed. */
export function buildPlan(env) {
  const mutating = env.RBXOC_SWEEP_MUTATE === "1";
  const requested = env.RBXOC_SWEEP_CASES?.split(",").map((s) => s.trim()).filter(Boolean);
  for (const label of requested ?? []) {
    if (!cases.some((c) => c.label === label)) throw new Error(`Unknown sweep case: ${label}`);
  }
  const selected = cases.filter((c) => requested ? requested.includes(c.label) : mutating || !c.mutating);
  const targets = {};
  const resolved = selected.map((c) => {
    if (c.mutating && !mutating) {
      throw new Error(`${c.label}: requires RBXOC_SWEEP_MUTATE=1; no command was run`);
    }
    const args = c.args.map((value) => {
      if (typeof value === "string") return value;
      const name = value.env;
      const raw = env[name];
      if (typeof raw !== "string" || !raw.trim()) throw new Error(`${c.label}: required env ${name} is missing`);
      if (name.endsWith("_ID") && !/^[1-9][0-9]*$/.test(raw)) {
        // JOB_ID is an opaque server identifier, not a numeric resource ID.
        if (name !== "JOB_ID") throw new Error(`${name} must be a positive numeric ID`);
      }
      if (/(?:_ID|_NAME|_SCOPE|_ENTRY|_TOPIC|_VERSION|_REPOSITORY|_FILE)$/.test(name)) targets[name] = raw;
      return raw;
    });
    return { ...c, args: ["--format", "json", ...args] };
  });
  return { mutating, targets, cases: resolved };
}
