// `node:zlib` over Rust host ops. Only the synchronous entry points the
// protocol layer calls.
//
// `zstdCompressSync` is deliberately absent: `util/ncbl.js` feature-detects it
// with `typeof zlib.zstdCompressSync === 'function'` and falls back to gzip,
// so leaving it undefined selects the fallback rather than breaking.

import { Buffer } from "./buffer.js";

const host = globalThis.__ncm_host;

const asBytes = (v) =>
  v instanceof Uint8Array ? v : ArrayBuffer.isView(v) ? new Uint8Array(v.buffer, v.byteOffset, v.byteLength) : Buffer.from(v);

export const gunzipSync = (data) => Buffer.from(host.gunzip(asBytes(data)));
export const gzipSync = (data) => Buffer.from(host.gzip(asBytes(data)));
export const inflateSync = (data) => Buffer.from(host.inflate(asBytes(data)));
export const deflateSync = (data) => Buffer.from(host.deflate(asBytes(data)));

export default { gunzipSync, gzipSync, inflateSync, deflateSync };
