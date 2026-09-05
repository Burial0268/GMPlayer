// Minimal Node `Buffer` over `Uint8Array`.
//
// Scope is deliberately the subset `util/crypto.js`, `util/request.js`,
// `util/ncbl.js` and the endpoint modules actually use — encodings
// base64/hex/utf8/latin1(binary), alloc/concat/compare, and the fixed-width
// integer accessors the NCBL log envelope reads and writes. It is NOT a general
// Buffer polyfill; adding surface here should be driven by a real call site, not
// by completeness.
//
// UTF-8 goes through Rust host ops rather than a hand-rolled encoder: the
// protocol signs UTF-8 bytes of JSON payloads that routinely contain CJK, and
// a subtly wrong surrogate path would produce a valid-looking but rejected
// signature.

const host = globalThis.__ncm_host;

const HEX = "0123456789abcdef";

const fromHex = (s) => {
  const clean = s.length % 2 ? "0" + s : s;
  const out = new Uint8Array(clean.length >> 1);
  for (let i = 0; i < out.length; i++) {
    const b = parseInt(clean.substr(i * 2, 2), 16);
    if (Number.isNaN(b)) return out.subarray(0, i);
    out[i] = b;
  }
  return out;
};

const toHex = (u8) => {
  let s = "";
  for (let i = 0; i < u8.length; i++) s += HEX[u8[i] >> 4] + HEX[u8[i] & 15];
  return s;
};

const fromLatin1 = (s) => {
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i) & 0xff;
  return out;
};

const toLatin1 = (u8) => {
  // Chunked: spreading a large array into String.fromCharCode blows the stack.
  let s = "";
  for (let i = 0; i < u8.length; i += 0x8000) {
    s += String.fromCharCode.apply(null, u8.subarray(i, i + 0x8000));
  }
  return s;
};

const fromBase64 = (s) => {
  const norm = String(s).replace(/-/g, "+").replace(/_/g, "/").replace(/[^A-Za-z0-9+/=]/g, "");
  return fromLatin1(atob(norm));
};

const toBase64 = (u8) => btoa(toLatin1(u8));

const normalizeEncoding = (enc) => {
  const e = String(enc || "utf8").toLowerCase();
  if (e === "utf-8" || e === "utf8") return "utf8";
  if (e === "binary" || e === "latin1") return "latin1";
  if (e === "base64" || e === "base64url") return "base64";
  if (e === "hex") return "hex";
  if (e === "ascii") return "latin1";
  throw new TypeError(`Unsupported encoding in embedded Buffer shim: ${enc}`);
};

class Buffer extends Uint8Array {
  static from(value, encodingOrOffset, length) {
    if (typeof value === "string") {
      switch (normalizeEncoding(encodingOrOffset)) {
        case "utf8":
          return new Buffer(host.utf8Encode(value));
        case "latin1":
          return new Buffer(fromLatin1(value));
        case "base64":
          return new Buffer(fromBase64(value));
        case "hex":
          return new Buffer(fromHex(value));
      }
    }
    if (value instanceof ArrayBuffer) {
      return new Buffer(
        encodingOrOffset === undefined
          ? new Uint8Array(value)
          : new Uint8Array(value, encodingOrOffset, length)
      );
    }
    if (ArrayBuffer.isView(value)) {
      return new Buffer(new Uint8Array(value.buffer, value.byteOffset, value.byteLength));
    }
    if (Array.isArray(value) || (value && typeof value.length === "number")) {
      return new Buffer(Uint8Array.from(value));
    }
    throw new TypeError("Buffer.from: unsupported input");
  }

  static alloc(size, fill = 0) {
    const b = new Buffer(new Uint8Array(size));
    if (fill) b.fill(fill);
    return b;
  }

  static allocUnsafe(size) {
    return Buffer.alloc(size);
  }

  static concat(list, totalLength) {
    const parts = list.map((p) => (p instanceof Uint8Array ? p : Buffer.from(p)));
    const total = totalLength ?? parts.reduce((n, p) => n + p.length, 0);
    const out = new Buffer(new Uint8Array(total));
    let off = 0;
    for (const p of parts) {
      if (off >= total) break;
      const take = Math.min(p.length, total - off);
      out.set(p.subarray(0, take), off);
      off += take;
    }
    return out;
  }

  static isBuffer(x) {
    return x instanceof Buffer;
  }

  static byteLength(value, encoding) {
    return typeof value === "string" ? Buffer.from(value, encoding).length : value.length;
  }

  toString(encoding, start, end) {
    const view = this.subarray(start ?? 0, end ?? this.length);
    switch (normalizeEncoding(encoding)) {
      case "utf8":
        return host.utf8Decode(view);
      case "latin1":
        return toLatin1(view);
      case "base64":
        return toBase64(view);
      case "hex":
        return toHex(view);
    }
  }

  toJSON() {
    return { type: "Buffer", data: Array.from(this) };
  }

  equals(other) {
    if (this.length !== other.length) return false;
    for (let i = 0; i < this.length; i++) if (this[i] !== other[i]) return false;
    return true;
  }

  copy(target, targetStart = 0, sourceStart = 0, sourceEnd = this.length) {
    const src = this.subarray(sourceStart, sourceEnd);
    target.set(src, targetStart);
    return src.length;
  }

  // Node's Buffer#slice is a *view*, unlike Array#slice. `subarray` already
  // produces one — it constructs through the species constructor, so it comes
  // back as a Buffer sharing this ArrayBuffer. Wrapping it in `new Buffer(...)`
  // would clone, quietly turning aliased writes into lost writes.
  slice(start, end) {
    return this.subarray(start, end);
  }

  readUInt8(o = 0) {
    return this[o];
  }
  writeUInt8(v, o = 0) {
    this[o] = v & 0xff;
    return o + 1;
  }
  readUInt32BE(o = 0) {
    return ((this[o] << 24) | (this[o + 1] << 16) | (this[o + 2] << 8) | this[o + 3]) >>> 0;
  }
  writeUInt32BE(v, o = 0) {
    this[o] = (v >>> 24) & 0xff;
    this[o + 1] = (v >>> 16) & 0xff;
    this[o + 2] = (v >>> 8) & 0xff;
    this[o + 3] = v & 0xff;
    return o + 4;
  }

  // Little-endian, for `util/ncbl.js` — the ChaCha20 state, the NCBL header and
  // every frame header behind `scrobble_v1`. Written by hand rather than through
  // a `DataView`: the nonce and each frame are `subarray` views, and a DataView
  // over `this.buffer` would ignore their `byteOffset` and read the wrong bytes.
  readUInt16LE(o = 0) {
    return this[o] | (this[o + 1] << 8);
  }
  writeUInt16LE(v, o = 0) {
    this[o] = v & 0xff;
    this[o + 1] = (v >>> 8) & 0xff;
    return o + 2;
  }
  readUInt32LE(o = 0) {
    return (this[o] | (this[o + 1] << 8) | (this[o + 2] << 16) | (this[o + 3] << 24)) >>> 0;
  }
  writeUInt32LE(v, o = 0) {
    this[o] = v & 0xff;
    this[o + 1] = (v >>> 8) & 0xff;
    this[o + 2] = (v >>> 16) & 0xff;
    this[o + 3] = (v >>> 24) & 0xff;
    return o + 4;
  }
}

export { Buffer };
export default { Buffer };
