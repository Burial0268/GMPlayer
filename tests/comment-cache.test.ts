import { test } from "node:test";
import assert from "node:assert/strict";
import { comment } from "@/api/comment";

// `@/utils/ncm/request` is replaced by the runner with a stub that calls this.
type Call = { url: string; params: Record<string, unknown> };
const calls: Call[] = [];
let answer: (call: Call) => Promise<unknown> = async () => ({ code: 200, comments: [] });
(globalThis as any).__commentRequest = (call: Call) => {
  calls.push(call);
  return answer(call);
};

test("one song's comments are fetched once and shared by every caller", async () => {
  calls.length = 0;
  const [a, b] = await Promise.all([comment.get(1), comment.get(1, 0, null, "music")]);
  assert.equal(calls.length, 1);
  assert.equal(a, b);
  await comment.get(1);
  assert.equal(calls.length, 1);
});

test("fresh skips the cache, and other pages or resource types are their own entry", async () => {
  calls.length = 0;
  await comment.get(1, 0, null, "music", true);
  await comment.get(1, 20);
  await comment.get(1, 0, null, "mv");
  assert.equal(calls.length, 3);
});

test("a failed or non-200 answer is not kept", async () => {
  calls.length = 0;
  answer = async () => ({ code: 405 });
  await comment.get(2);
  await comment.get(2);
  answer = async () => {
    throw new Error("offline");
  };
  await assert.rejects(comment.get(3));
  answer = async () => ({ code: 200, comments: [] });
  await comment.get(3);
  assert.equal(calls.length, 4);
});

test("liking an MV comment sends the MV resource type", async () => {
  calls.length = 0;
  await comment.like(9, 7, 1, "mv");
  await comment.like(9, 7, 1);
  assert.deepEqual(
    calls.map((c) => c.params.type),
    [1, 0],
  );
});
