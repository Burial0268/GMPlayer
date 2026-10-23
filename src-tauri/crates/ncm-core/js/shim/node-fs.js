// `node:fs` for the embedded build.
//
// The protocol layer touches the filesystem in exactly three places:
//
//   util/index.js    reads data/china_ip_ranges.txt   → bundled in as an asset
//   util/request.js  reads/writes <tmp>/anonymous_token
//   util/request.js  reads/writes <tmp>/xeapi_public_key
//
// The first is static data, inlined at bundle time. The other two are session
// state that must survive across calls and, ideally, across restarts — the
// xeapi public key is fetched from the server and re-fetching it on every
// launch is a wasted round trip. Both are delegated to the Rust side, which
// owns where (and whether) they land on disk.
//
// Everything else — streams, promises, stat — belongs to the upload helpers,
// which the embedded build does not support. Those throw rather than returning
// plausible empty values.

import { Buffer } from "./buffer.js";
import { ASSETS } from "./generated-data.js";

const host = globalThis.__ncm_host;

const unsupported = (fn) => () => {
  throw new Error(
    `fs.${fn} is not available in the embedded NCM protocol runtime ` +
      `(file upload endpoints are unsupported; see scripts/build-ncm-protocol.mjs)`
  );
};

/** Map a path onto either a bundled asset or a Rust-backed state key. */
const classify = (p) => {
  const norm = String(p).replace(/\\/g, "/");
  for (const name of Object.keys(ASSETS)) {
    if (norm.endsWith(name)) return { kind: "asset", name };
  }
  const base = norm.slice(norm.lastIndexOf("/") + 1);
  if (base === "anonymous_token" || base === "xeapi_public_key") {
    return { kind: "state", name: base };
  }
  return { kind: "unknown", name: norm };
};

export const readFileSync = (p, options) => {
  const enc = typeof options === "string" ? options : options && options.encoding;
  const target = classify(p);
  let text;
  if (target.kind === "asset") {
    text = ASSETS[target.name];
  } else if (target.kind === "state") {
    text = host.stateRead(target.name) ?? "";
  } else {
    const err = new Error(`ENOENT: no such file or directory, open '${p}'`);
    err.code = "ENOENT";
    throw err;
  }
  return enc ? text : Buffer.from(text, "utf8");
};

export const writeFileSync = (p, data) => {
  const target = classify(p);
  if (target.kind !== "state") {
    throw new Error(`fs.writeFileSync to '${p}' is not permitted in the embedded runtime`);
  }
  host.stateWrite(target.name, data instanceof Uint8Array ? Buffer.from(data).toString("utf8") : String(data));
};

export const existsSync = (p) => {
  const target = classify(p);
  if (target.kind === "asset") return true;
  if (target.kind === "state") return host.stateRead(target.name) != null;
  return false;
};

export const promises = {
  stat: unsupported("promises.stat"),
  unlink: unsupported("promises.unlink"),
  open: unsupported("promises.open"),
  readFile: async (p, o) => readFileSync(p, o),
  writeFile: async (p, d) => writeFileSync(p, d),
};

export const createReadStream = unsupported("createReadStream");
export const createWriteStream = unsupported("createWriteStream");

export default {
  readFileSync,
  writeFileSync,
  existsSync,
  promises,
  createReadStream,
  createWriteStream,
};
