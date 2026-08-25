// Bundles the NeteaseCloudMusic protocol layer into a single script that runs
// inside the embedded QuickJS isolate (`src-tauri/crates/ncm-core`).
//
// What this produces is NOT a server. `server.js`, express, the proxy agents
// and the CLI are all dropped; what survives is the endpoint modules plus the
// weapi/eapi/xeapi request envelope, with every Node primitive redirected to a
// Rust host op. See docs/native-ncm-api-embedding-plan.md.
//
//   pnpm build:ncm-protocol           # rebuild from the pinned tarball
//   pnpm build:ncm-protocol --check   # verify the committed output is current
//
// The output IS committed, so a plain `cargo build` never needs Node. CI runs
// --check to guarantee the committed artifact matches this script's output.

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";

// ── Pinned upstream ──────────────────────────────────────────────
// Bump deliberately, never with a range. `integrity` is npm's published
// dist.integrity for this exact version; a mismatch aborts the build rather
// than silently baking in a different protocol implementation.
const PKG_NAME = "@neteasecloudmusicapienhanced/api";
const PKG_VERSION = "4.40.1";
const PKG_INTEGRITY =
  "sha512-RUpVnxUCkeEt0yYeElc+UvCXqucikI/PIlJaLj18FlwHQ8X9wk4QkM/h7C+qePC97COMODcmgRXKgWO94YLuWA==";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "..");
const CRATE = path.join(REPO, "src-tauri", "crates", "ncm-core");
const SHIM = path.join(CRATE, "js", "shim");
const CACHE = path.join(CRATE, ".cache");
const PKG_DIR = path.join(CACHE, `api-${PKG_VERSION}`);
const OUT_FILE = path.join(CRATE, "js", "ncm-protocol.js");

const checkOnly = process.argv.includes("--check");

// ── Module resolution policy ─────────────────────────────────────
// Every bare import the bundle can reach must land in exactly one bucket.
// Anything unlisted is a hard error: silently externalizing a dependency would
// produce a bundle that only fails at call time, inside the isolate, with no
// stack worth reading.

/** Node builtins and npm packages replaced by a shim backed by Rust host ops. */
const SHIMMED = {
  crypto: "node-crypto.js",
  "node:crypto": "node-crypto.js",
  zlib: "node-zlib.js",
  "node:zlib": "node-zlib.js",
  fs: "node-fs.js",
  "node:fs": "node-fs.js",
  path: "node-path.js",
  "node:path": "node-path.js",
  os: "node-os.js",
  "node:os": "node-os.js",
  url: "node-url.js",
  "node:url": "node-url.js",
  http: "node-http.js",
  "node:http": "node-http.js",
  https: "node-http.js",
  "node:https": "node-http.js",
  buffer: "buffer.js",
  "node:buffer": "buffer.js",
  axios: "axios.js",
  "node-forge": "node-forge.js",
  dotenv: "dotenv.js",
};

/**
 * Dependencies with no embedded equivalent. They resolve to a stub that throws
 * a named error when touched, so an unsupported endpoint reports *which*
 * capability it wanted instead of a bare ReferenceError.
 *
 * Each entry lists the endpoints it takes down, so the blast radius of a stub
 * is reviewable here rather than discoverable in production.
 */
const UNAVAILABLE = {
  jsdom: "register/checktoken/v2 (NetEase Yidun anti-cheat, needs a DOM)",
  "music-metadata": "cloud upload metadata probing",
  xml2js: "voice/upload",
  qrcode: "login/qr/create image rendering — the frontend renders the qrurl itself",
  "pac-proxy-agent": "PAC proxy support — configure the proxy on the Rust HTTP client",
  tunnel: "HTTP CONNECT proxy support — configure it on the Rust HTTP client",
  "@neteasecloudmusicapienhanced/unblockmusic-utils":
    "in-package UNM unlocking — this app uses its own UNM service (VITE_UNM_API)",
  express: "the HTTP server shell, which the embedded build does not run",
  "express-fileupload": "the HTTP server shell",
  "safe-decode-uri-component": "the HTTP server shell",
  yargs: "the CLI shell",
  child_process: "subprocess spawning",
  "node:child_process": "subprocess spawning",
  stream: "Node streams",
  "node:stream": "Node streams",
  net: "raw sockets",
  tls: "raw TLS sockets",
};

/** Bundled verbatim from the repo's own node_modules. */
const VENDORED = new Set(["crypto-js"]);

// ── Fetch + verify + extract ─────────────────────────────────────

async function ensurePackage() {
  if (fs.existsSync(path.join(PKG_DIR, "package.json"))) return;

  fs.mkdirSync(CACHE, { recursive: true });
  const tarball = `https://registry.npmjs.org/${PKG_NAME}/-/${PKG_NAME.split("/")[1]}-${PKG_VERSION}.tgz`;
  process.stderr.write(`  fetching ${PKG_NAME}@${PKG_VERSION}\n`);

  const res = await fetch(tarball);
  if (!res.ok) throw new Error(`tarball fetch failed: ${res.status} ${res.statusText}`);
  const bytes = Buffer.from(await res.arrayBuffer());

  const [algo, expected] = PKG_INTEGRITY.split("-");
  const actual = createHash(algo).update(bytes).digest("base64");
  if (actual !== expected) {
    throw new Error(
      `integrity mismatch for ${PKG_NAME}@${PKG_VERSION}\n` +
        `  expected ${algo}-${expected}\n  actual   ${algo}-${actual}`
    );
  }

  const tgz = path.join(CACHE, `api-${PKG_VERSION}.tgz`);
  fs.writeFileSync(tgz, bytes);
  fs.mkdirSync(PKG_DIR, { recursive: true });
  try {
    // `tar` ships with Windows 10+, macOS and every CI image we build on.
    // Paths are passed relative to `cwd`: an absolute Windows path like
    // `H:\...` reads as a `host:path` remote spec to MSYS/GNU tar and fails
    // with "Cannot connect to H".
    execFileSync("tar", ["xzf", path.join("..", path.basename(tgz)), "--strip-components=1"], {
      cwd: PKG_DIR,
      stdio: "pipe",
    });
  } catch (e) {
    throw new Error(`failed to extract ${tgz} — is \`tar\` on PATH?\n${e.message}`);
  }
  fs.rmSync(tgz, { force: true });

  // 14 MB of demo docs we will never read.
  fs.rmSync(path.join(PKG_DIR, "public"), { recursive: true, force: true });
}

// ── Entry generation ─────────────────────────────────────────────

/**
 * Static data files the protocol layer reads through `fs`, inlined so the
 * embedded runtime needs no real filesystem. Keyed by the suffix
 * `shim/node-fs.js` matches paths against.
 *
 * `china_ip_ranges.txt` is deliberately inlined **empty**. Upstream reads it to
 * mint a random Chinese IP for `X-Real-IP` / `X-Forwarded-For`, which only
 * makes sense for a *deployed server* proxying on someone's behalf. We are the
 * client: Netease sees the user's own address on the socket, and a forwarding
 * header claiming a different one is a mismatch their risk control can act on.
 * Nothing here sets `realIP`, so the header is never emitted — the table was
 * 68 KB of bundle re-parsed into ~9k CIDR ranges on every isolate start, for a
 * value that was already unused. The upstream loader treats an empty file as
 * "no ranges" without erroring.
 */
function generateAssets() {
  const assets = {
    "china_ip_ranges.txt": "",
  };
  const out = path.join(SHIM, "generated-data.js");
  fs.writeFileSync(
    out,
    "// GENERATED by scripts/build-ncm-protocol.mjs — do not edit.\n" +
      "// Static assets the protocol layer reads via fs, inlined for the embedded runtime.\n" +
      `export const ASSETS = ${JSON.stringify(assets)};\n` +
      "export default ASSETS;\n"
  );
  return Object.fromEntries(Object.entries(assets).map(([k, v]) => [k, v.length]));
}

/**
 * Every endpoint module in the package, not just the ones `src/api/*.ts`
 * currently calls.
 *
 * A whitelist was the original design, but it cannot be derived reliably:
 * several call sites build the path dynamically (`/comment/${type}`,
 * `/toplist${detail ? "/detail" : ""}`), so a scanner either misses them or
 * needs a hand-maintained exception list that silently rots. Bundling all of
 * them costs far less than that drift — the endpoint modules are thin
 * parameter shaping over a shared request core.
 */
function generateEntry() {
  const moduleDir = path.join(PKG_DIR, "module");
  const names = fs
    .readdirSync(moduleDir)
    .filter((f) => f.endsWith(".js"))
    .map((f) => f.slice(0, -3))
    .sort();

  const lines = [
    "// GENERATED by scripts/build-ncm-protocol.mjs — do not edit.",
    `import ${JSON.stringify(path.join(SHIM, "prelude.js").replace(/\\/g, "/"))};`,
    'const { cookieToJson } = require("./util/index.js");',
    'const { getXeapiPublicKey } = require("./util/xeapiKey.js");',
    'const request = require("./util/request.js");',
    "",
    // Endpoint modules load on first call, never at startup. Two reasons:
    //
    //  1. Some modules run code at import time (`voice_upload` does
    //     `new xml2js.Parser()`), and those that depend on a stubbed package
    //     would take the whole isolate down during evaluation — including for
    //     callers that never touch them.
    //  2. Evaluating 439 modules to serve one endpoint is wasted startup.
    "const loaders = {",
    ...names.map((n) => `  ${JSON.stringify(n)}: () => require("./module/${n}.js"),`),
    "};",
    "",
    "const loaded = Object.create(null);",
    "const load = (name) => {",
    "  if (!(name in loaded)) loaded[name] = loaders[name]();",
    "  return loaded[name];",
    "};",
    "",
    // Mirrors main.js's per-call wrapper: normalize the cookie into the object
    // form the modules expect, then hand them the shared request function.
    //
    // The result is JSON, and the promise never rejects. `util/request.js`
    // rejects with its `answer` object for any non-200 upstream code, but those
    // are real responses the caller acts on (301 = not logged in, and so on) —
    // surfacing them as isolate errors would lose the body.
    "const normalize = (r, ok) => {",
    "  try {",
    "    if (r && typeof r === 'object' && 'body' in r) {",
    "      return JSON.stringify({",
    "        ok,",
    "        status: typeof r.status === 'number' ? r.status : ok ? 200 : 500,",
    "        body: r.body,",
    "        cookie: r.cookie || [],",
    "      });",
    "    }",
    "    const msg = r && r.message ? r.message : String(r);",
    "    return JSON.stringify({ ok: false, status: 500, body: { code: 500, msg }, cookie: [] });",
    "  } catch (e) {",
    "    // A body that will not serialize (cycles, BigInt) must still produce a",
    "    // well-formed envelope rather than taking down the isolate.",
    "    return JSON.stringify({",
    "      ok: false,",
    "      status: 500,",
    "      body: { code: 500, msg: 'response could not be serialized: ' + (e && e.message) },",
    "      cookie: [],",
    "    });",
    "  }",
    "};",
    "",
    "globalThis.__ncmProtocol = {",
    "  version: " + JSON.stringify(PKG_VERSION) + ",",
    "  endpoints: Object.keys(loaders),",
    "  call(name, queryJson) {",
    "    if (!Object.hasOwn(loaders, name)) {",
    "      return Promise.resolve(",
    "        normalize({ status: 404, body: { code: 404, msg: 'unknown endpoint: ' + name } }, false)",
    "      );",
    "    }",
    "    let q;",
    "    try {",
    "      q = queryJson ? JSON.parse(queryJson) : {};",
    "    } catch (e) {",
    "      return Promise.resolve(",
    "        normalize({ status: 400, body: { code: 400, msg: 'malformed query JSON' } }, false)",
    "      );",
    "    }",
    "    const cookie =",
    "      typeof q.cookie === 'string' ? cookieToJson(q.cookie) : q.cookie || {};",
    "    try {",
    "      // load() is inside the try: a module that throws while being",
    "      // evaluated (an unavailable dependency at import time) must report",
    "      // as a failed call, not escape into the host.",
    "      const mod = load(name);",
    "      return Promise.resolve(mod({ ...q, cookie }, request)).then(",
    "        (r) => normalize(r, true),",
    "        (e) => normalize(e, false)",
    "      );",
    "    } catch (e) {",
    "      return Promise.resolve(normalize(e, false));",
    "    }",
    "  },",
    "",
    // Equivalent of the upstream `generateConfig.js`, which the deployed server
    // runs once at startup. The embedded runtime has no such step, so it does
    // the same work itself.
    //
    // The ordering is subtle and worth stating, because it looks wrong:
    //
    //   * The xeapi key request needs a deviceId.
    //   * The only thing that mints a deviceId is `register_anonimous`, which
    //     publishes it to `globalThis.deviceId` *before* issuing its own
    //     request — and that request is itself xeapi, so on cold state it
    //     fails for want of the very key we are trying to fetch.
    //
    // So the first anonymous attempt is expected to fail; it runs for its side
    // effect. Upstream gets the same result by accident, converging only on a
    // second process launch. Doing both passes here makes a cold start work in
    // one go.
    "  async bootstrap() {",
    "    const report = { xeapiKey: null, anonymousToken: null };",
    "",
    "    const anonymous = async () => {",
    "      try {",
    "        const res = await load('register_anonimous')({ cookie: {} }, request);",
    "        const raw = res && res.body && res.body.cookie;",
    "        const obj = raw ? cookieToJson(raw) : {};",
    "        if (!obj.MUSIC_A) return 'no MUSIC_A in response';",
    "        globalThis.__ncm_host.stateWrite('anonymous_token', obj.MUSIC_A);",
    "        return 'ok';",
    "      } catch (e) {",
    "        return 'failed: ' + (e && e.message ? e.message : String(e));",
    "      }",
    "    };",
    "",
    "    report.anonymousToken = await anonymous();",
    "",
    "    try {",
    "      let current = {};",
    "      const cached = globalThis.__ncm_host.stateRead('xeapi_public_key');",
    "      if (cached) {",
    "        try {",
    "          current = JSON.parse(cached);",
    "        } catch (e) {}",
    "      }",
    "      const pk = await getXeapiPublicKey(current, globalThis.deviceId || '');",
    "      globalThis.__ncm_host.stateWrite('xeapi_public_key', JSON.stringify(pk));",
    "      report.xeapiKey = 'ok';",
    "    } catch (e) {",
    "      report.xeapiKey = 'failed: ' + (e && e.message ? e.message : String(e));",
    "    }",
    "",
    "    if (report.xeapiKey === 'ok' && report.anonymousToken !== 'ok') {",
    "      report.anonymousToken = await anonymous();",
    "    }",
    "",
    "    return JSON.stringify(report);",
    "  },",
    "};",
    "",
  ];

  const entry = path.join(PKG_DIR, "__ncm_entry.js");
  fs.writeFileSync(entry, lines.join("\n"));
  return { entry, count: names.length };
}

// ── esbuild plugin: route every bare import ──────────────────────

const routingPlugin = {
  name: "ncm-host-routing",
  setup(b) {
    b.onResolve({ filter: /.*/ }, (args) => {
      // Relative and absolute paths resolve normally.
      if (args.path.startsWith(".") || path.isAbsolute(args.path)) return null;

      if (Object.hasOwn(SHIMMED, args.path)) {
        return { path: path.join(SHIM, SHIMMED[args.path]) };
      }
      if (Object.hasOwn(UNAVAILABLE, args.path)) {
        // Keyed by the module's own specifier, not a shared shim path:
        // esbuild caches onLoad results per (path, namespace), so reusing one
        // path would collapse every stub into whichever one loaded first —
        // `require('dotenv')` would silently come back as the qrcode stub.
        return { path: args.path, namespace: "ncm-unavailable" };
      }
      if (VENDORED.has(args.path) || args.path.startsWith("crypto-js/")) return null;

      return {
        errors: [
          {
            text:
              `unrouted dependency "${args.path}" (imported from ${path.relative(PKG_DIR, args.importer)}).\n` +
              `Add it to SHIMMED, UNAVAILABLE or VENDORED in scripts/build-ncm-protocol.mjs — ` +
              `leaving it unrouted would build a bundle that only fails inside the isolate.`,
          },
        ],
      };
    });

    b.onLoad({ filter: /.*/, namespace: "ncm-unavailable" }, (args) => ({
      contents:
        `import { unavailable } from ${JSON.stringify(path.join(SHIM, "unavailable.js").replace(/\\/g, "/"))};\n` +
        `module.exports = unavailable(${JSON.stringify(args.path)}, ${JSON.stringify(UNAVAILABLE[args.path])});\n`,
      loader: "js",
      resolveDir: SHIM,
    }));
  },
};

// ── Main ─────────────────────────────────────────────────────────

async function main() {
  await ensurePackage();
  const assets = generateAssets();
  const { entry, count } = generateEntry();

  const result = await build({
    entryPoints: [entry],
    bundle: true,
    format: "iife",
    platform: "browser",
    target: "es2023",
    minify: true,
    write: false,
    metafile: true,
    legalComments: "none",
    // crypto-js probes for `global` when deciding how to source randomness.
    // `__dirname` only ever reaches `path.join(__dirname, '../data/...')`, and
    // the fs shim matches assets by path suffix, so any stable value works —
    // but it has to exist, or the CIDR table silently fails to load and the
    // realIP header falls back.
    define: {
      global: "globalThis",
      __dirname: '"/ncm"',
      __filename: '"/ncm/index.js"',
      "process.env.NODE_ENV": '"production"',
    },
    plugins: [routingPlugin],
  });

  const banner =
    `// GENERATED — do not edit. Source: ${PKG_NAME}@${PKG_VERSION}\n` +
    `// Rebuild: pnpm build:ncm-protocol\n`;
  const code = banner + result.outputFiles[0].text;
  const digest = createHash("sha256").update(code).digest("hex");

  if (checkOnly) {
    if (!fs.existsSync(OUT_FILE)) {
      console.error("✘ ncm-protocol.js is missing. Run: pnpm build:ncm-protocol");
      process.exit(1);
    }
    const committed = fs.readFileSync(OUT_FILE, "utf8");
    if (committed !== code) {
      console.error(
        "✘ ncm-protocol.js is stale — it does not match what this script produces.\n" +
          "  Run: pnpm build:ncm-protocol"
      );
      process.exit(1);
    }
    console.error(`✔ ncm-protocol.js is current (${count} endpoints, sha256 ${digest.slice(0, 12)})`);
    return;
  }

  fs.mkdirSync(path.dirname(OUT_FILE), { recursive: true });
  fs.writeFileSync(OUT_FILE, code);

  const gz = (await import("node:zlib")).gzipSync(Buffer.from(code)).length;
  const assetBytes = Object.values(assets).reduce((a, b) => a + b, 0);
  console.error(
    `✔ ${path.relative(REPO, OUT_FILE)}\n` +
      `  ${count} endpoints | ${code.length} B | ${gz} B gzip | sha256 ${digest.slice(0, 12)}\n` +
      `  inlined assets: ${Object.keys(assets).join(", ")} (${assetBytes} B)`
  );
}

main().catch((e) => {
  console.error(`✘ ${e.message}`);
  process.exit(1);
});
