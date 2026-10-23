// `node:url` — re-exports the globals installed by prelude.js so that
// `require('url')` and the bare globals stay the same implementation. Two
// URLSearchParams classes would be a subtle trap: `util/request.js` builds the
// POST body with one of them, and a divergence in form encoding is invisible
// locally and rejected on the wire.

export const URL = globalThis.URL;
export const URLSearchParams = globalThis.URLSearchParams;

export const parse = (input) => new globalThis.URL(input);
export const format = (u) => String(u);
export const fileURLToPath = (u) => new globalThis.URL(u).pathname;

export default { URL, URLSearchParams, parse, format, fileURLToPath };
