// Break a WebView2 (or Chromium) process's memory down by allocator, over time.
//
// A heap snapshot sees only the main thread's V8 heap; this asks the browser for
// memory-infra dumps, which also cover workers, PartitionAlloc, malloc, Skia,
// the image cache, shared memory and so on — per process.
//
// Needs the app started with `--remote-debugging-port=9222` (debug builds do;
// see DEFAULT_ADDITIONAL_WINDOW_ARGS) and Node 22+ (global WebSocket).
//
//   node scripts/webview-memory-dump.mjs [port=9222] [intervalSeconds=60] [count=10]
//
// Prints every process's allocators in MB at each dump, with the change since
// the first one, largest first.

const [port = "9222", intervalArg = "60", countArg = "10"] = process.argv.slice(2);
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

let first = null;
for (let i = 0; i < count; i++) {
  const { names, perProcess } = await dump();
  first ??= perProcess;
  console.log(`\n=== dump ${i + 1}/${count} at ${new Date().toLocaleTimeString()} ===`);
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
  if (i + 1 < count) await new Promise((resolve) => setTimeout(resolve, interval));
}
ws.close();
