// TEMP(skip-trace): diagnostics for the in-app skip's playState round trip; delete with the step-3 fix.
// One tag so a single console filter (`skipTrace`) shows the whole skip in order. Objects are
// stringified so a copied console log keeps their values instead of a collapsed `Object`.
export const skipTrace = (...args: unknown[]): void => {
  if (!import.meta.env?.DEV) return;
  const text = args.map((arg) => (typeof arg === "object" ? JSON.stringify(arg) : String(arg)));
  console.log(`[skipTrace ${performance.now().toFixed(0)}]`, text.join(" "));
};
