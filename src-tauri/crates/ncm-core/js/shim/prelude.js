// Globals QuickJS does not provide, kept to the subset the NCM protocol layer
// actually touches. Probed empirically against rquickjs 0.12 / quickjs-ng:
//
//   present : atob btoa BigInt Uint8Array Promise queueMicrotask Proxy Reflect
//             Date RegExp Map Set WeakMap JSON Math Symbol Error
//   missing : URL URLSearchParams TextEncoder TextDecoder console Buffer
//             setTimeout structuredClone crypto fetch
//
// `crypto` and `fetch` are deliberately NOT polyfilled — the protocol layer
// reaches those through the `node:crypto` / axios shims, which route to Rust
// host ops. Anything that tries to use them as globals should fail loudly.

import { Buffer } from "./buffer.js";

const host = globalThis.__ncm_host;

// ── console ──────────────────────────────────────────────────────
// The package's `util/logger.js` and a handful of stray `console.log`/
// `console.error` calls need this. Routed to the Rust logger so protocol
// diagnostics land in the same place as the rest of the backend.
//
// Secrets discipline: the Rust side redacts before it writes. Do not add a
// fast path here that bypasses it.
const fmt = (args) =>
  args
    .map((a) => {
      if (typeof a === "string") return a;
      if (a instanceof Error) return a.stack || a.message;
      try {
        return JSON.stringify(a);
      } catch {
        return String(a);
      }
    })
    .join(" ");

globalThis.console = {
  log: (...a) => host.log("info", fmt(a)),
  info: (...a) => host.log("info", fmt(a)),
  warn: (...a) => host.log("warn", fmt(a)),
  error: (...a) => host.log("error", fmt(a)),
  debug: (...a) => host.log("debug", fmt(a)),
  trace: (...a) => host.log("debug", fmt(a)),
};

// ── TextEncoder / TextDecoder ────────────────────────────────────
// UTF-8 only, which is all the protocol layer uses.
globalThis.TextEncoder = class TextEncoder {
  get encoding() {
    return "utf-8";
  }
  encode(input = "") {
    return host.utf8Encode(String(input));
  }
};

globalThis.TextDecoder = class TextDecoder {
  constructor(label = "utf-8") {
    this.encoding = String(label).toLowerCase();
  }
  decode(input) {
    if (input == null) return "";
    const bytes = input instanceof Uint8Array ? input : new Uint8Array(input.buffer || input);
    return host.utf8Decode(bytes);
  }
};

// ── Buffer ───────────────────────────────────────────────────────
globalThis.Buffer = Buffer;

// ── process ──────────────────────────────────────────────────────
// The protocol layer reads `process.env` for optional proxy settings and
// feature flags. There is no process environment here, so the object exists
// and is empty rather than absent — every read must miss, not throw.
//
// `env` is a plain object on purpose: `dotenv`'s no-op shim writes nothing,
// and nothing else in the bundle mutates it.
globalThis.process = {
  env: Object.create(null),
  platform: "linux",
  arch: "x64",
  version: "v20.0.0",
  versions: { node: "20.0.0" },
  argv: [],
  cwd: () => "/ncm-state",
  nextTick: (fn, ...args) => queueMicrotask(() => fn(...args)),
  // Anything reaching this would be `server.js` shutting the process down,
  // and the embedded build does not run it. Failing loudly beats a silent
  // no-op that leaves the caller believing it exited.
  exit: () => {
    throw new Error("process.exit is not available in the embedded NCM protocol runtime");
  },
};

// ── setTimeout ───────────────────────────────────────────────────
// Nothing on the request path schedules real time: the HTTP host op blocks,
// so axios-style timeout timers never need to fire. Provide the API so a
// stray call does not throw, but run the callback as a microtask rather than
// pretending to sleep — a shim that silently delayed would stall the isolate's
// worker thread for the caller's timeout budget.
let timerSeq = 1;
const cancelled = new Set();

globalThis.setTimeout = (fn, _ms, ...args) => {
  const id = timerSeq++;
  queueMicrotask(() => {
    if (cancelled.has(id)) {
      cancelled.delete(id);
      return;
    }
    try {
      fn(...args);
    } catch (e) {
      console.error("setTimeout callback threw:", e);
    }
  });
  return id;
};
globalThis.clearTimeout = (id) => {
  cancelled.add(id);
};
globalThis.setImmediate = (fn, ...args) => globalThis.setTimeout(fn, 0, ...args);
globalThis.clearImmediate = globalThis.clearTimeout;

// ── URLSearchParams ──────────────────────────────────────────────
// `util/request.js` builds the POST body with this, and it is the one shim
// whose output goes on the wire verbatim. Encoding must match Node exactly:
// application/x-www-form-urlencoded with space as '+'.
const encodeFormComponent = (v) =>
  encodeURIComponent(String(v))
    .replace(/%20/g, "+")
    .replace(/[!'()~]/g, (c) => "%" + c.charCodeAt(0).toString(16).toUpperCase());

globalThis.URLSearchParams = class URLSearchParams {
  #pairs = [];

  constructor(init) {
    if (init == null) return;
    if (typeof init === "string") {
      for (const chunk of init.replace(/^\?/, "").split("&")) {
        if (!chunk) continue;
        const eq = chunk.indexOf("=");
        const k = eq < 0 ? chunk : chunk.slice(0, eq);
        const v = eq < 0 ? "" : chunk.slice(eq + 1);
        this.#pairs.push([decodeURIComponent(k.replace(/\+/g, " ")), decodeURIComponent(v.replace(/\+/g, " "))]);
      }
    } else if (Array.isArray(init)) {
      for (const [k, v] of init) this.#pairs.push([String(k), String(v)]);
    } else if (init instanceof globalThis.URLSearchParams) {
      for (const [k, v] of init.entries()) this.#pairs.push([k, v]);
    } else {
      // Node stringifies every own enumerable value, including `undefined` and
      // `null` — `new URLSearchParams({level: undefined})` yields
      // `level=undefined`, not an omitted key. Skipping them here would look
      // tidier and would change the bytes on the wire: `buildXeapiPlaintext`
      // base64s this output into the signed payload, so any divergence from
      // what the deployed server sends today is a silent protocol change.
      for (const k of Object.keys(init)) {
        this.#pairs.push([String(k), String(init[k])]);
      }
    }
  }

  append(k, v) {
    this.#pairs.push([String(k), String(v)]);
  }
  set(k, v) {
    const i = this.#pairs.findIndex(([pk]) => pk === String(k));
    if (i < 0) this.#pairs.push([String(k), String(v)]);
    else {
      this.#pairs[i][1] = String(v);
      this.#pairs = this.#pairs.filter(([pk], j) => pk !== String(k) || j === i);
    }
  }
  get(k) {
    const hit = this.#pairs.find(([pk]) => pk === String(k));
    return hit ? hit[1] : null;
  }
  getAll(k) {
    return this.#pairs.filter(([pk]) => pk === String(k)).map(([, v]) => v);
  }
  has(k) {
    return this.#pairs.some(([pk]) => pk === String(k));
  }
  delete(k) {
    this.#pairs = this.#pairs.filter(([pk]) => pk !== String(k));
  }
  entries() {
    return this.#pairs.map((p) => [p[0], p[1]])[Symbol.iterator]();
  }
  keys() {
    return this.#pairs.map((p) => p[0])[Symbol.iterator]();
  }
  values() {
    return this.#pairs.map((p) => p[1])[Symbol.iterator]();
  }
  forEach(fn, thisArg) {
    for (const [k, v] of this.#pairs) fn.call(thisArg, v, k, this);
  }
  [Symbol.iterator]() {
    return this.entries();
  }
  get size() {
    return this.#pairs.length;
  }
  toString() {
    return this.#pairs
      .map(([k, v]) => `${encodeFormComponent(k)}=${encodeFormComponent(v)}`)
      .join("&");
  }
};

// ── URL ──────────────────────────────────────────────────────────
// Only reached by the proxy branch of `util/request.js`, which the embedded
// build disables. Parses enough to keep that code path from throwing.
globalThis.URL = class URL {
  constructor(input, base) {
    const raw = base ? new URL(base).origin + String(input) : String(input);
    const m = raw.match(/^([a-zA-Z][a-zA-Z0-9+.-]*:)\/\/([^/?#]*)([^?#]*)(\?[^#]*)?(#.*)?$/);
    if (!m) throw new TypeError(`Invalid URL: ${raw}`);
    this.protocol = m[1];
    const auth = m[2];
    const at = auth.lastIndexOf("@");
    const creds = at < 0 ? "" : auth.slice(0, at);
    const hostPart = at < 0 ? auth : auth.slice(at + 1);
    const colon = creds.indexOf(":");
    this.username = colon < 0 ? creds : creds.slice(0, colon);
    this.password = colon < 0 ? "" : creds.slice(colon + 1);
    const hc = hostPart.lastIndexOf(":");
    const bracket = hostPart.lastIndexOf("]");
    if (hc > bracket) {
      this.hostname = hostPart.slice(0, hc);
      this.port = hostPart.slice(hc + 1);
    } else {
      this.hostname = hostPart;
      this.port = "";
    }
    this.host = hostPart;
    this.pathname = m[3] || "/";
    this.search = m[4] || "";
    this.hash = m[5] || "";
    this.origin = `${this.protocol}//${this.host}`;
    this.searchParams = new globalThis.URLSearchParams(this.search);
  }
  get href() {
    return `${this.origin}${this.pathname}${this.search}${this.hash}`;
  }
  toString() {
    return this.href;
  }
};

export {};
