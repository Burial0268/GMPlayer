// `node:crypto` over Rust host ops.
//
// Surface is exactly what the protocol layer calls, no more:
//
//   weapi  : createHash('md5')
//   eapi   : createHash('md5'), aes-128-ecb
//   xeapi  : aes-{128,256}-ecb, aes-128-gcm, createHmac('sha256'),
//            generateKeyPairSync('x25519'), diffieHellman, createPublicKey
//   misc   : randomBytes, randomUUID, createHash('sha256')
//
// The streaming Cipher/Hash API is emulated by buffering in `update()` and
// doing the work in `final()`/`digest()`. Every call site in the package is a
// single update followed by a final, so this costs one extra copy and buys a
// far smaller op surface than a real streaming binding.
//
// Padding note: Node defaults to PKCS#7 auto-padding for block modes, and the
// package never calls `setAutoPadding(false)`. The ECB ops below pad/unpad to
// match — getting this wrong produces ciphertext the server rejects with a
// generic error, so it is asserted in the crate's Rust tests instead of being
// discovered on the wire.

import { Buffer } from "./buffer.js";

const host = globalThis.__ncm_host;

const asBytes = (v, encoding) => {
  if (v == null) return new Uint8Array(0);
  if (v instanceof Uint8Array) return v;
  if (ArrayBuffer.isView(v)) return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  if (v instanceof ArrayBuffer) return new Uint8Array(v);
  if (typeof v === "string") return Buffer.from(v, encoding || "utf8");
  throw new TypeError("crypto shim: expected string or bytes");
};

const encodeDigest = (bytes, encoding) =>
  encoding ? Buffer.from(bytes).toString(encoding) : Buffer.from(bytes);

// ── Hash / HMAC ──────────────────────────────────────────────────

class Hash {
  #algo;
  #chunks = [];
  constructor(algo) {
    this.#algo = String(algo).toLowerCase().replace("-", "");
    if (this.#algo !== "md5" && this.#algo !== "sha256") {
      throw new Error(`crypto shim: unsupported hash "${algo}"`);
    }
  }
  update(data, inputEncoding) {
    this.#chunks.push(asBytes(data, inputEncoding));
    return this;
  }
  digest(encoding) {
    const input = Buffer.concat(this.#chunks);
    const out = this.#algo === "md5" ? host.md5(input) : host.sha256(input);
    return encodeDigest(out, encoding);
  }
}

class Hmac {
  #key;
  #chunks = [];
  constructor(algo, key) {
    const a = String(algo).toLowerCase().replace("-", "");
    if (a !== "sha256") throw new Error(`crypto shim: unsupported hmac "${algo}"`);
    this.#key = asBytes(key);
  }
  update(data, inputEncoding) {
    this.#chunks.push(asBytes(data, inputEncoding));
    return this;
  }
  digest(encoding) {
    const out = host.hmacSha256(this.#key, Buffer.concat(this.#chunks));
    return encodeDigest(out, encoding);
  }
}

// ── Ciphers ──────────────────────────────────────────────────────

const parseAlgo = (algo) => {
  const m = String(algo)
    .toLowerCase()
    .match(/^aes-(128|192|256)-(ecb|cbc|gcm)$/);
  if (!m) throw new Error(`crypto shim: unsupported cipher "${algo}"`);
  return { bits: Number(m[1]), mode: m[2] };
};

class Cipher {
  #mode;
  #key;
  #iv;
  #chunks = [];
  #tag = null;
  #done = false;

  constructor(algo, key, iv, decrypt) {
    const { bits, mode } = parseAlgo(algo);
    this.#mode = mode;
    this.#key = asBytes(key);
    this.#iv = iv == null ? null : asBytes(iv);
    this.decrypt = decrypt;
    if (this.#key.length * 8 !== bits) {
      throw new Error(
        `crypto shim: ${algo} needs a ${bits / 8}-byte key, got ${this.#key.length}`
      );
    }
  }

  setAutoPadding(on) {
    if (!on) throw new Error("crypto shim: setAutoPadding(false) is not implemented");
    return this;
  }

  setAuthTag(tag) {
    this.#tag = asBytes(tag);
    return this;
  }

  getAuthTag() {
    if (this.#tag == null) throw new Error("crypto shim: getAuthTag() before final()");
    return Buffer.from(this.#tag);
  }

  update(data, inputEncoding, _outputEncoding) {
    this.#chunks.push(asBytes(data, inputEncoding));
    // Everything is emitted from final(); see the header note.
    return Buffer.alloc(0);
  }

  final(outputEncoding) {
    if (this.#done) throw new Error("crypto shim: final() called twice");
    this.#done = true;
    const input = Buffer.concat(this.#chunks);
    let out;
    if (this.#mode === "ecb") {
      out = this.decrypt
        ? host.aesEcbDecrypt(this.#key, input)
        : host.aesEcbEncrypt(this.#key, input);
    } else if (this.#mode === "cbc") {
      out = this.decrypt
        ? host.aesCbcDecrypt(this.#key, this.#iv, input)
        : host.aesCbcEncrypt(this.#key, this.#iv, input);
    } else {
      if (this.decrypt) {
        if (this.#tag == null) throw new Error("crypto shim: GCM decrypt needs setAuthTag()");
        out = host.aesGcmDecrypt(this.#key, this.#iv, input, this.#tag);
      } else {
        const r = host.aesGcmEncrypt(this.#key, this.#iv, input);
        this.#tag = r.tag;
        out = r.ciphertext;
      }
    }
    return outputEncoding ? Buffer.from(out).toString(outputEncoding) : Buffer.from(out);
  }
}

// ── X25519 ───────────────────────────────────────────────────────
// Node models these as opaque KeyObjects. The package only ever round-trips
// them through SPKI DER, so the shim keeps the raw 32 bytes and synthesizes
// the RFC 8410 header on export.

const X25519_SPKI_PREFIX = new Uint8Array([
  0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x6e, 0x03, 0x21, 0x00,
]);

class KeyObject {
  constructor(type, raw) {
    this.type = type;
    this.asymmetricKeyType = "x25519";
    this._raw = raw;
  }
  export(opts = {}) {
    if (opts.format === "der" && opts.type === "spki") {
      return Buffer.concat([Buffer.from(X25519_SPKI_PREFIX), Buffer.from(this._raw)]);
    }
    if (opts.format === "raw" || (!opts.format && !opts.type)) {
      return Buffer.from(this._raw);
    }
    throw new Error(
      `crypto shim: unsupported key export ${JSON.stringify(opts)} (only der/spki and raw)`
    );
  }
}

const rawFromKeyInput = (input) => {
  if (input instanceof KeyObject) return input._raw;
  const key = input && input.key !== undefined ? input.key : input;
  const bytes = asBytes(key);
  // Accept both bare 32-byte keys and SPKI DER; the trailing 32 bytes are the
  // curve point either way.
  return bytes.length === 32 ? bytes : bytes.subarray(bytes.length - 32);
};

// ── Exports ──────────────────────────────────────────────────────

export const randomBytes = (size) => Buffer.from(host.randomBytes(size));
export const randomUUID = () => host.randomUuid();
export const createHash = (algo) => new Hash(algo);
export const createHmac = (algo, key) => new Hmac(algo, key);
export const createCipheriv = (algo, key, iv) => new Cipher(algo, key, iv, false);
export const createDecipheriv = (algo, key, iv) => new Cipher(algo, key, iv, true);

export const createPublicKey = (input) => new KeyObject("public", rawFromKeyInput(input));
export const createPrivateKey = (input) => new KeyObject("private", rawFromKeyInput(input));

export const generateKeyPairSync = (type) => {
  if (String(type).toLowerCase() !== "x25519") {
    throw new Error(`crypto shim: unsupported key type "${type}"`);
  }
  const kp = host.x25519Generate();
  return {
    publicKey: new KeyObject("public", kp.publicKey),
    privateKey: new KeyObject("private", kp.privateKey),
  };
};

export const diffieHellman = ({ privateKey, publicKey }) =>
  Buffer.from(host.x25519Diffie(rawFromKeyInput(privateKey), rawFromKeyInput(publicKey)));

// `getRandomValues` is how crypto-js sources entropy when it decides it is in
// a browser; providing it keeps crypto-js off its `require('crypto')` path.
export const getRandomValues = (typed) => {
  const bytes = host.randomBytes(typed.byteLength);
  new Uint8Array(typed.buffer, typed.byteOffset, typed.byteLength).set(bytes);
  return typed;
};

export const webcrypto = { getRandomValues, randomUUID };

export default {
  randomBytes,
  randomUUID,
  createHash,
  createHmac,
  createCipheriv,
  createDecipheriv,
  createPublicKey,
  createPrivateKey,
  generateKeyPairSync,
  diffieHellman,
  getRandomValues,
  webcrypto,
};
