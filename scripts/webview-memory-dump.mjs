// Break a WebView2 (or Chromium) process's memory down by allocator, over time.
//
// A heap snapshot sees only the main thread's V8 heap; this asks the browser for
// memory-infra dumps, which also cover workers, PartitionAlloc, malloc, Skia,
// the image cache, shared memory and so on — per process.
//
// Needs the app started with `--remote-debugging-port=9222` (debug builds do;
// see DEFAULT_ADDITIONAL_WINDOW_ARGS) and Node 22+ (global WebSocket).
//
//   node scripts/webview-memory-dump.mjs [port=9222] [intervalSeconds=60] [count=10] [gc]
//
// Prints every process's allocators in MB at each dump, with the change since
// the first one, largest first. With `gc`, each dump is followed by a forced
// garbage collection in every page and a second dump: what drops was garbage
// waiting for a collection, what stays is retained.

const [port = "9222", intervalArg = "60", countArg = "10", gcArg] = process.argv.slice(2);
const interval = Number(intervalArg) * 1000;
const count = Number(countArg);

const version = await fetch(`http://127.0.0.1:${port}/json/version`).then((r) => r.json());
const ws = new WebSocket(version.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.onopen = resolve;
  ws.onerror = () => reject(new Error(`cannot connect to the debugging port ${port}`));
});

let nextId = 0;
const replies = new Map();
let events = [];
let onComplete = () => {};
ws.onmessage = ({ data }) => {
  const msg = JSON.parse(data);
  if (msg.id && replies.has(msg.id)) {
    replies.get(msg.id)(msg);
    replies.delete(msg.id);
  } else if (msg.method === "Tracing.dataCollected") events.push(...msg.params.value);
  else if (msg.method === "Tracing.tracingComplete") onComplete();
};
const send = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const id = ++nextId;
    replies.set(id, (msg) => (msg.error ? reject(new Error(`${method}: ${msg.error.message}`)) : resolve(msg.result)));
    ws.send(JSON.stringify({ id, method, params }));
  });

const mb = (hex) => parseInt(hex, 16) / 1048576;

async function dump() {
  events = [];
  const complete = new Promise((resolve) => (onComplete = resolve));
  await send("Tracing.start", {
    transferMode: "ReportEvents",
    traceConfig: {
      includedCategories: ["disabled-by-default-memory-infra"],
      excludedCategories: ["*"],
      memoryDumpConfig: { triggers: [] },
    },
  });
  await send("Tracing.requestMemoryDump", { deterministic: true, levelOfDetail: "detailed" });
  await send("Tracing.end");
  await complete;

  const names = {};
  const perProcess = {};
  for (const e of events) {
    if (e.ph === "M" && e.name === "process_name") names[e.pid] = e.args.name;
    if (e.ph === "M" && e.name === "process_labels") names[e.pid] = `${names[e.pid] ?? ""} ${e.args.labels}`;
    const dumps = e.ph === "v" && e.args?.dumps;
    if (!dumps) continue;
    const sizes = (perProcess[e.pid] ??= {});
    const totals = dumps.process_totals;
    if (totals?.private_footprint_bytes) sizes["(private footprint)"] = mb(totals.private_footprint_bytes);
    // Allocator dumps are a tree; take each root's own size, else the sum of its direct children.
    const roots = {};
    for (const [path, entry] of Object.entries(dumps.allocators ?? {})) {
      const size = entry.attrs?.effective_size?.value ?? entry.attrs?.size?.value;
      if (!size) continue;
      const parts = path.split("/");
      const root = (roots[parts[0]] ??= { own: null, children: 0 });
      if (parts.length === 1) root.own = mb(size);
      else if (parts.length === 2) root.children += mb(size);
    }
    for (const [name, { own, children }] of Object.entries(roots)) sizes[name] = own ?? children;
  }
  return { names, perProcess };
}

// Full GC (V8 and Blink's heap together) in every page, each over its own target connection.
async function collectGarbage() {
  const targets = await fetch(`http://127.0.0.1:${port}/json/list`).then((r) => r.json());
  for (const target of targets.filter((t) => t.type === "page" && t.webSocketDebuggerUrl)) {
    const page = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => {
      page.onopen = resolve;
      page.onerror = reject;
    }).catch(() => null);
    if (page.readyState !== WebSocket.OPEN) continue;
    await new Promise((resolve) => {
      page.onmessage = resolve;
      page.send(JSON.stringify({ id: 1, method: "HeapProfiler.collectGarbage" }));
    });
    page.close();
  }
}

let first = null;
for (let i = 0; i < count * (gcArg === "gc" ? 2 : 1); i++) {
  const afterGc = gcArg === "gc" && i % 2 === 1;
  if (afterGc) await collectGarbage();
  const { names, perProcess } = await dump();
  first ??= perProcess;
  const round = gcArg === "gc" ? Math.floor(i / 2) : i;
  console.log(`\n=== dump ${round + 1}/${count}${gcArg === "gc" ? (afterGc ? " after GC" : " before GC") : ""} at ${new Date().toLocaleTimeString()} ===`);
  for (const [pid, sizes] of Object.entries(perProcess)) {
    const footprint = sizes["(private footprint)"] ?? 0;
    console.log(`\n[pid ${pid}] ${(names[pid] ?? "").trim()}  private footprint ${footprint.toFixed(0)} MB`);
    const rows = Object.entries(sizes)
      .filter(([name]) => name !== "(private footprint)")
      .map(([name, size]) => [name, size, size - (first[pid]?.[name] ?? size)])
      .sort((a, b) => b[1] - a[1])
      .slice(0, 12);
    for (const [name, size, delta] of rows) {
      const change = delta ? ` (${delta > 0 ? "+" : ""}${delta.toFixed(1)})` : "";
      console.log(`  ${size.toFixed(1).padStart(8)} MB  ${name}${change}`);
    }
  }
  const lastRound = (gcArg === "gc" ? Math.floor(i / 2) : i) + 1 >= count;
  if (!lastRound && (gcArg !== "gc" || afterGc)) await new Promise((resolve) => setTimeout(resolve, interval));
}
ws.close();
