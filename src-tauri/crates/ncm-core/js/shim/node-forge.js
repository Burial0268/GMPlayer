// `node-forge`, reduced to the one operation the protocol layer uses.
//
// util/crypto.js:
//   const forgePublicKey = forge.pki.publicKeyFromPem(key)
//   const encrypted = forgePublicKey.encrypt(str, 'NONE')
//   return forge.util.bytesToHex(encrypted)
//
// That is textbook RSA with no padding: left-pad the message to the modulus
// width, then m^e mod n. forge is 286 KB of the 402 KB bundle to provide it,
// which is why it is a shim rather than a dependency.
//
// The PEM parse happens in Rust (`rsaRawPem`) rather than here — walking the
// SPKI DER in JS to pull out n and e would be another 60 lines of ASN.1 for no
// benefit, and Rust already needs a bigint for the modexp.
//
// Scheme handling is strict: anything other than 'NONE' throws. Silently
// applying PKCS#1 padding when a caller asked for something else would produce
// a request the server rejects with a generic error.

import { Buffer } from "./buffer.js";

const host = globalThis.__ncm_host;

const HEX = "0123456789abcdef";

class PublicKey {
  constructor(pem) {
    this.pem = pem;
  }
  encrypt(message, scheme) {
    if (scheme !== "NONE") {
      throw new Error(
        `node-forge shim: only the 'NONE' (raw) scheme is implemented, got ${JSON.stringify(scheme)}`
      );
    }
    // forge takes and returns binary strings, and util/crypto.js feeds it one.
    const input = typeof message === "string" ? Buffer.from(message, "latin1") : Buffer.from(message);
    const out = host.rsaRawPem(this.pem, input);
    return Buffer.from(out).toString("latin1");
  }
}

export const pki = {
  publicKeyFromPem: (pem) => new PublicKey(String(pem)),
};

export const util = {
  bytesToHex: (bytes) => {
    const u8 = typeof bytes === "string" ? Buffer.from(bytes, "latin1") : Buffer.from(bytes);
    let s = "";
    for (let i = 0; i < u8.length; i++) s += HEX[u8[i] >> 4] + HEX[u8[i] & 15];
    return s;
  },
  hexToBytes: (hex) => Buffer.from(String(hex), "hex").toString("latin1"),
  encode64: (bytes) => Buffer.from(String(bytes), "latin1").toString("base64"),
  decode64: (b64) => Buffer.from(String(b64), "base64").toString("latin1"),
};

export default { pki, util };
