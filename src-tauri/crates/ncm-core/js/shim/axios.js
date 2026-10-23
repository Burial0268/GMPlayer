// axios over the Rust HTTP host op.
//
// `util/request.js` is the only hot caller and it uses a narrow slice of the
// axios contract:
//
//   axios({ method, url, headers, data, timeout, responseType })
//     → { status, headers, data }
//
// Two behaviours have to match the real client exactly, because
// `util/request.js` branches on them:
//
//   * `responseType: 'arraybuffer'` must yield a Buffer. The eapi/xeapi paths
//     call `body.toString('hex')` / `Buffer.from(body)` on it.
//   * Otherwise axios auto-parses JSON, and the caller tests
//     `typeof body === 'object' ? body : parse(body.toString())`. Returning a
//     Buffer here would satisfy `typeof === 'object'` and hand the caller raw
//     bytes as if they were a parsed response — silently wrong, not an error.
//
//   * `res.headers['set-cookie']` must be an array; the caller `.map()`s it.
//
// `util/ncbl.js` is the other caller, and it is the reason `validateStatus` is
// honoured below rather than hardcoded.
//
// ## This file is CommonJS, deliberately
//
// Every other shim here is ESM, and this one cannot be. The upstream package is
// CJS, so it reaches this through `require('axios')` — and esbuild answers a
// `require` of an *ESM* module with a namespace object, which is not callable.
// Nine of the ten call sites write `const { default: axios } = require('axios')`
// and never notice; `util/ncbl.js` writes `const axios = require('axios')` and
// calls it, which is what `scrobble_v1` is built on. That call was the second
// half of its `502 请求异常: not a function` — QuickJS's message for invoking a
// non-callable, thrown here on `axios(...)` rather than anywhere near the crypto
// the message makes you suspect.
//
// Exporting the function as `module.exports` satisfies both spellings at once:
// `require('axios')` is the function, and `.default` on it is the same function.
// Do not convert this file back to `export default` for consistency with its
// neighbours — `util/request.js` would keep working and `scrobble_v1` would
// silently break again.

const { Buffer } = require("./buffer.js");

const host = globalThis.__ncm_host;

class AxiosError extends Error {
  constructor(message, response) {
    super(message);
    this.name = "AxiosError";
    this.isAxiosError = true;
    this.response = response;
  }
}

const normalizeHeaders = (headers) => {
  const out = {};
  for (const [k, v] of Object.entries(headers || {})) {
    if (v === undefined || v === null) continue;
    out[String(k)] = Array.isArray(v) ? v.map(String) : String(v);
  }
  return out;
};

// axios fills in a Content-Type when the caller does not, and the protocol
// layer leans on that: `module/register_xeapikey.js` posts a
// URLSearchParams-encoded string with only User-Agent and Cookie set, and the
// weapi branch of `util/request.js` sets one only for xeapi. Without this the
// server answers 400 (key handshake) or an empty 200 (weapi) — failures that
// look like a broken crypto envelope rather than a missing header.
const FORM_TYPE = "application/x-www-form-urlencoded;charset=utf-8";

const applyDefaultContentType = (headers, data) => {
  if (data == null) return;
  if (Object.keys(headers).some((k) => k.toLowerCase() === "content-type")) return;
  headers["Content-Type"] = typeof data === "string" ? FORM_TYPE : "application/json";
};

// Which statuses resolve instead of rejecting.
//
// The default is axios's, and `util/request.js` relies on the rejection to
// produce its 502 answer. But `util/ncbl.js`'s NCBL log upload — the transport
// behind `scrobble_v1` — passes `() => true` on purpose, because it reads the
// refusal's *body* to report which of PLV/PLD the server declined. Ignoring the
// option collapsed every one of those into the endpoint's generic
// `请求异常: Request failed with status code …`, which names nothing about which
// upload failed or why. `null` means "accept everything" in axios; a function
// means ask it.
const accepts = (validateStatus, status) => {
  if (validateStatus === null) return true;
  if (typeof validateStatus === "function") return Boolean(validateStatus(status));
  return status >= 200 && status < 300;
};

function request(config) {
  const cfg = typeof config === "string" ? { url: config } : config || {};
  const method = String(cfg.method || "GET").toUpperCase();
  const wantsBytes = cfg.responseType === "arraybuffer";

  let body = null;
  if (cfg.data != null) {
    body =
      cfg.data instanceof Uint8Array
        ? cfg.data
        : Buffer.from(typeof cfg.data === "string" ? cfg.data : JSON.stringify(cfg.data));
  }

  const headers = normalizeHeaders(cfg.headers);
  applyDefaultContentType(headers, cfg.data);

  // `httpRequest` returns a real promise backed by an async Rust future, so the
  // isolate stays free to run other jobs while this is outstanding. That is
  // what lets a page load fan out a dozen endpoint calls instead of
  // serializing them behind one blocking socket.
  return host
    .httpRequest({
      method,
      url: String(cfg.url),
      headers,
      body,
      timeoutMs: Number(cfg.timeout) > 0 ? Number(cfg.timeout) : 0,
    })
    .then(
      (raw) => {
        const resHeaders = raw.headers || {};
        let data;
        if (wantsBytes) {
          data = Buffer.from(raw.body);
        } else {
          const text = Buffer.from(raw.body).toString("utf8");
          try {
            data = JSON.parse(text);
          } catch {
            data = text;
          }
        }

        const response = {
          status: raw.status,
          statusText: raw.statusText || "",
          headers: resHeaders,
          data,
          config: cfg,
        };

        // axios rejects on non-2xx by default, and `util/request.js` relies on
        // that to produce its 502 answer.
        if (!accepts(cfg.validateStatus, raw.status)) {
          throw new AxiosError(`Request failed with status code ${raw.status}`, response);
        }
        return response;
      },
      (e) => {
        throw new AxiosError(String(e && e.message ? e.message : e), undefined);
      },
    );
}

const axios = Object.assign(request, {
  request,
  get: (url, cfg) => request({ ...cfg, url, method: "GET" }),
  delete: (url, cfg) => request({ ...cfg, url, method: "DELETE" }),
  head: (url, cfg) => request({ ...cfg, url, method: "HEAD" }),
  post: (url, data, cfg) => request({ ...cfg, url, data, method: "POST" }),
  put: (url, data, cfg) => request({ ...cfg, url, data, method: "PUT" }),
  create: () => axios,
  defaults: { headers: { common: {} } },
  interceptors: {
    request: { use: () => 0, eject: () => {} },
    response: { use: () => 0, eject: () => {} },
  },
  AxiosError,
  isAxiosError: (e) => Boolean(e && e.isAxiosError),
});

axios.default = axios;

module.exports = axios;
module.exports.axios = axios;
module.exports.AxiosError = AxiosError;
