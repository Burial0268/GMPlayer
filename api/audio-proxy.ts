import { Readable } from "node:stream";

function isBlockedHost(hostname: string): boolean {
  const host = hostname.toLowerCase();
  if (
    host === "localhost" ||
    host.endsWith(".localhost") ||
    host.endsWith(".local") ||
    host === "0.0.0.0" ||
    host === "127.0.0.1" ||
    host === "::1"
  ) {
    return true;
  }
  if (/^127\./.test(host) || /^10\./.test(host) || /^192\.168\./.test(host)) {
    return true;
  }
  return /^172\.(1[6-9]|2\d|3[0-1])\./.test(host);
}

export default async function handler(req: any, res: any) {
  if (req.method !== "GET" && req.method !== "HEAD") {
    res.status(405).send("method not allowed");
    return;
  }

  const rawTarget = Array.isArray(req.query?.url) ? req.query.url[0] : req.query?.url;
  if (!rawTarget) {
    res.status(400).send("missing url");
    return;
  }

  let target: URL;
  try {
    target = new URL(rawTarget);
  } catch {
    res.status(400).send("invalid url");
    return;
  }

  if (
    (target.protocol !== "http:" && target.protocol !== "https:") ||
    isBlockedHost(target.hostname)
  ) {
    res.status(400).send("unsupported url");
    return;
  }

  try {
    const headers = new Headers({
      Accept: "audio/*,*/*;q=0.8",
      Referer: `${target.origin}/`,
      "User-Agent":
        req.headers["user-agent"] || "Mozilla/5.0 AppleWebKit/537.36 Chrome Safari",
    });
    const range = req.headers.range;
    if (range) headers.set("Range", range);

    const upstream = await fetch(target, {
      headers,
      method: req.method === "HEAD" ? "HEAD" : "GET",
      redirect: "follow",
    });

    res.status(upstream.status);
    for (const header of [
      "accept-ranges",
      "cache-control",
      "content-length",
      "content-range",
      "content-type",
      "etag",
      "last-modified",
    ]) {
      const value = upstream.headers.get(header);
      if (value) res.setHeader(header, value);
    }
    res.setHeader("x-audio-source-url", upstream.url || target.href);

    if (req.method === "HEAD" || !upstream.body) {
      res.end();
      return;
    }

    Readable.fromWeb(upstream.body as any).pipe(res);
  } catch (err) {
    res.status(502).send(err instanceof Error ? err.message : String(err));
  }
}
