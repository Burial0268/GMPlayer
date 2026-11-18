import assert from "node:assert/strict";
import { test } from "node:test";
import {
  bassSwapValueAt,
  buildEqCurve,
  easedProgress,
  eqLeanAt,
  getBalancedCrossfadeGains,
  getCrossfadeValues,
} from "../src/utils/AudioContext/AutoMix/curves";

const close = (a: number, b: number, eps = 1e-9) => Math.abs(a - b) < eps;

test("the eased progress is monotone, exact at its ends, and leaves and arrives at rest", () => {
  assert.equal(easedProgress(0), 0);
  assert.equal(easedProgress(1), 1);
  assert.ok(close(easedProgress(0.5), 0.5));
  let last = 0;
  for (let i = 1; i <= 1000; i++) {
    const w = easedProgress(i / 1000);
    assert.ok(w >= last);
    last = w;
  }
  const d = 1e-5;
  assert.ok(easedProgress(d) / d < 1e-3);
  assert.ok((1 - easedProgress(1 - d)) / d < 1e-3);
});

test("every curve starts and ends exactly, at rest, with constant power where it should", () => {
  for (const curve of ["linear", "equalPower", "sCurve"] as const) {
    for (const [inShape, outShape] of [
      [1, 1],
      [1.3, 0.7],
    ]) {
      assert.deepEqual(getCrossfadeValues(0, curve, inShape, outShape), [1, 0]);
      assert.deepEqual(getCrossfadeValues(1, curve, inShape, outShape), [0, 1]);
      const d = 1e-4;
      const [out0, in0] = getCrossfadeValues(d, curve, inShape, outShape);
      const [out1, in1] = getCrossfadeValues(1 - d, curve, inShape, outShape);
      for (const slope of [(1 - out0) / d, in0 / d, out1 / d, (1 - in1) / d]) {
        assert.ok(slope < 0.05, `${curve} ${inShape}/${outShape}: slope ${slope}`);
      }
      if (curve !== "linear") {
        for (let i = 0; i <= 20; i++) {
          const [o, n] = getCrossfadeValues(i / 20, curve, inShape, outShape);
          assert.ok(close(o * o + n * n, 1, 1e-9));
        }
      }
    }
  }
});

test("balanced gains go from A's target to B's without a corner at either end", () => {
  const gains = (t: number) => getBalancedCrossfadeGains(t, "equalPower", 1, 1, 0.8, 1.1, -1.2);
  const [outStart, inStart] = gains(0);
  const [outEnd, inEnd] = gains(1);
  assert.ok(close(outStart, 0.8, 1e-12) && close(inStart, 0));
  assert.ok(close(outEnd, 0, 1e-12) && close(inEnd, 1.1, 1e-12));
  const d = 1e-4;
  assert.ok(Math.abs(gains(d)[0] - outStart) / d < 0.05);
  assert.ok(Math.abs(inEnd - gains(1 - d)[1]) / d < 0.05);
});

test("EQ curves hold, ease and resume where they left off", () => {
  assert.equal(bassSwapValueAt(0.3, -6, 2), -6);
  assert.equal(bassSwapValueAt(0.7, -6, 2), 2);
  assert.ok(close(bassSwapValueAt(0.5, -6, 2), -2));
  assert.equal(eqLeanAt(0, 0, 4), 0);
  assert.equal(eqLeanAt(1, 0, 4), 4);

  // A resumed fade continues the same curve: the remainder of the full curve, sample for sample.
  const valueAt = (p: number) => bassSwapValueAt(p, 0, -24);
  const resumed = buildEqCurve(101, 0.45, 1, valueAt);
  assert.ok(close(resumed[0], valueAt(0.45)));
  assert.ok(close(resumed[100], -24));
  assert.ok(resumed[10] < resumed[0], "still mid-swap after resuming at 45%");
});
