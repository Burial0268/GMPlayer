/** Positions in stable snapshot order; additions keep their current order at the end. */
export const naturalQueueOrder = (
  list: readonly { id: number }[],
  snapshot: readonly number[],
): number[] => {
  if (!snapshot.length) return Array.from({ length: list.length }, (_, index) => index);

  const rank = new Map<number, number>();
  for (const id of snapshot) {
    if (!rank.has(id)) rank.set(id, rank.size);
  }
  const added = rank.size;
  const offsets = new Uint32Array(added + 1);
  for (const song of list) offsets[rank.get(song.id) ?? added]++;

  // Counting ranks avoids sorting the same references with repeated Map lookups.
  let offset = 0;
  for (let i = 0; i < offsets.length; i++) {
    const count = offsets[i];
    offsets[i] = offset;
    offset += count;
  }
  // oxlint-disable-next-line unicorn/no-new-array -- Every slot is filled below.
  const next = new Array<number>(list.length);
  let index = 0;
  for (const song of list) next[offsets[rank.get(song.id) ?? added]++] = index++;
  return next;
};
