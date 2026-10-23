// `node:path` — POSIX-only, which is all the bundle needs. Paths inside the
// embedded runtime are virtual keys (see node-fs.js), never real filesystem
// locations, so Windows drive/UNC handling would be dead weight.

const normalizeParts = (parts, allowAboveRoot) => {
  const out = [];
  for (const part of parts) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (out.length && out[out.length - 1] !== "..") out.pop();
      else if (allowAboveRoot) out.push("..");
      continue;
    }
    out.push(part);
  }
  return out;
};

export const sep = "/";
export const delimiter = ":";

export const join = (...segments) => {
  const joined = segments.filter((s) => s && s.length).join("/");
  if (!joined) return ".";
  const absolute = joined.startsWith("/");
  const normalized = normalizeParts(joined.split("/"), !absolute).join("/");
  return (absolute ? "/" : "") + normalized || (absolute ? "/" : ".");
};

export const resolve = (...segments) => {
  let resolved = "";
  let absolute = false;
  for (let i = segments.length - 1; i >= 0 && !absolute; i--) {
    const seg = segments[i];
    if (!seg) continue;
    resolved = resolved ? `${seg}/${resolved}` : seg;
    absolute = seg.startsWith("/");
  }
  const normalized = normalizeParts(resolved.split("/"), !absolute).join("/");
  return absolute ? "/" + normalized : normalized || ".";
};

export const dirname = (p) => {
  const s = String(p).replace(/\/+$/, "");
  const i = s.lastIndexOf("/");
  if (i < 0) return ".";
  if (i === 0) return "/";
  return s.slice(0, i);
};

export const basename = (p, ext) => {
  const s = String(p).replace(/\/+$/, "");
  let base = s.slice(s.lastIndexOf("/") + 1);
  if (ext && base.endsWith(ext) && base !== ext) base = base.slice(0, -ext.length);
  return base;
};

export const extname = (p) => {
  const base = basename(p);
  const i = base.lastIndexOf(".");
  return i <= 0 ? "" : base.slice(i);
};

export const isAbsolute = (p) => String(p).startsWith("/");
export const normalize = (p) => join(p);

export const posix = { sep, delimiter, join, resolve, dirname, basename, extname, isAbsolute, normalize };

export default posix;
