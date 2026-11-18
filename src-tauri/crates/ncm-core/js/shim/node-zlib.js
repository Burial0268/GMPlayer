// `node:zlib` over Rust host ops. Only the synchronous entry points the
// protocol layer calls.
//
// `zstdCompressSync` is what `util/ncbl.js` feature-detects for the NCBL
// scrobble body. Without it that falls back to gzip, which the upload endpoint
// accepts and never counts: the header has no codec flag, and the desktop client
// it imitates writes zstd.

import { Buffer } from "./buffer.js";

const host = globalThis.__ncm_host;

const asBytes = (v) =>
  v instanceof Uint8Array ? v : ArrayBuffer.isView(v) ? new Uint8Array(v.buffer, v.byteOffset, v.byteLength) : Buffer.from(v);

export const gunzipSync = (data) => Buffer.from(host.gunzip(asBytes(data)));
export const gzipSync = (data) => Buffer.from(host.gzip(asBytes(data)));
export const inflateSync = (data) => Buffer.from(host.inflate(asBytes(data)));
export const deflateSync = (data) => Buffer.from(host.deflate(asBytes(data)));
export const zstdCompressSync = (data) => Buffer.from(host.zstd(asBytes(data)));

export default { gunzipSync, gzipSync, inflateSync, deflateSync, zstdCompressSync };
