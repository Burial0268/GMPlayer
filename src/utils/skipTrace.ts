// TEMP(skip-trace): diagnostics for the in-app skip's playState round trip; delete with the step-3 fix.
// One tag so a single console filter (`skipTrace`) shows the whole skip in order.
export const skipTrace = (...args: unknown[]): void => {
  if (import.meta.env?.DEV) console.log(`[skipTrace ${performance.now().toFixed(0)}]`, ...args);
};
