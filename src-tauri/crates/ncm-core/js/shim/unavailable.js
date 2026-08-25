// Stub factory for dependencies with no embedded equivalent.
//
// The goal is a *named* failure. An endpoint that reaches one of these should
// report which capability it wanted and why it is absent, at the moment it is
// called — not fail later with a ReferenceError from inside minified code, and
// not silently return an empty value that looks like a legitimate "no result"
// response from Netease.
//
// The proxy shape matters: these modules are consumed as `require('x')`,
// `const { y } = require('x')` and `x.y()`, so property access has to survive
// long enough to reach the call.

export function unavailable(name, why) {
  // Must be a `function`, not an arrow. A Proxy's `construct` trap only runs
  // when the *target* has a [[Construct]] slot; with an arrow target,
  // `new xml2js.Parser()` throws a bare "not a constructor" TypeError before
  // the trap is ever consulted, losing the diagnostic entirely.
  function fail() {
    throw new Error(`"${name}" is not available in the embedded NCM protocol runtime: ${why}`);
  }

  return new Proxy(fail, {
    get(_target, prop) {
      if (prop === "__ncmUnavailable") return name;
      // Let interop and diagnostics probes through without detonating.
      if (prop === "then" || prop === Symbol.toPrimitive || prop === Symbol.toStringTag) {
        return undefined;
      }
      if (prop === "default") return unavailable(name, why);
      // Nested proxy rather than the bare `fail`: call sites reach for
      // `xml2js.Parser`, `dotenv.config` and so on, and each of those has to
      // stay both callable and constructible so the error names the full path.
      return unavailable(`${name}.${String(prop)}`, why);
    },
    apply: fail,
    construct: fail,
  });
}

export default unavailable;
