/**
 * AMLL's own line breaking, ported from @applemusic-like-lyrics/core 0.6.0
 * (`isCJK`, `chunkAndSplitLyricWords`, `calcBalancedBreaks`, `LineBalancer`),
 * none of which it exports. AMLL does not let CSS wrap a lyric: it measures
 * the line and inserts `<br>`s that even out the rows, preferring breaks after
 * punctuation and spaces and avoiding ones inside a CJK word. Anything that
 * stands in for an AMLL line has to break where it does. Re-port this when
 * AMLL is upgraded.
 */

/** A piece AMLL lays out as one unit: a word chunk (or segment), or a space. */
export interface BreakUnit {
  text: string;
  isSpace: boolean;
}

const isCJK = (text: string) => /^[\p{Unified_Ideograph}ࠀ-鿼]+$/u.test(text);

const OVERFLOW_PENALTY_MULTIPLIER = 1e3;
const CJK_BREAK_PENALTY_RATIO = 0.15;
const NORMAL_BREAK_PENALTY_RATIO = 0.5;
const SPACE_BREAK_REWARD_RATIO = 0.4;
const PUNCTUATION_BREAK_REWARD_RATIO = 0.6;
const PUNCTUATION_REGEX = /[,.;:!?，。；：！？、）】》」』’”)[\]}>~…]$/;

/**
 * The units AMLL builds a line from. A lyric without word timing is plain
 * text cut by the word segmenter; a timed one is AMLL's word chunks: spaces
 * and CJK characters on their own, pieces with no space between them joined.
 */
export function lineUnits(
  words: { word: string; romanWord?: string }[],
  plainText: boolean,
  segmenter: Intl.Segmenter,
): BreakUnit[] {
  if (plainText) {
    const text = words.map((w) => w.word).join("");
    return [...segmenter.segment(text)].map(({ segment }) => ({
      text: segment,
      isSpace: segment.trim().length === 0,
    }));
  }
  const units: BreakUnit[] = [];
  let group = "";
  const flush = () => {
    if (group) units.push({ text: group, isSpace: false });
    group = "";
  };
  const atom = (text: string) => {
    const isSpace = text.trim().length === 0;
    if (!isSpace && !isCJK(text)) group += text;
    else {
      flush();
      units.push({ text, isSpace });
    }
  };
  for (const { word, romanWord = "" } of words) {
    for (const part of word.split(/(\s+)/).filter(Boolean)) {
      // A CJK run is laid out per character unless it carries a romanisation.
      if (part.trim() && isCJK(part) && part.length > 1 && !romanWord.trim()) {
        for (const char of part) atom(char);
      } else atom(part);
    }
  }
  flush();
  return units;
}

/** Indices of the units AMLL puts a `<br>` before. */
export function balancedBreaks(
  units: BreakUnit[],
  widths: number[],
  containerWidth: number,
  segmenter: Intl.Segmenter,
): number[] {
  const n = units.length;
  if (n === 0 || containerWidth <= 0) return [];
  const fullText = units.map((u) => u.text).join("");
  const cjkBoundaries = new Set<number>();
  let offset = 0;
  for (const { segment, isWordLike } of segmenter.segment(fullText)) {
    if (offset > 0 && isWordLike && [...segment].some((ch) => isCJK(ch))) {
      cjkBoundaries.add(offset);
    }
    offset += segment.length;
  }
  const charOffsets = new Int32Array(n + 1);
  const prefixWidth = new Float64Array(n + 1);
  for (let i = 0; i < n; i++) {
    charOffsets[i + 1] = charOffsets[i] + units[i].text.length;
    prefixWidth[i + 1] = prefixWidth[i] + widths[i];
  }
  if (prefixWidth[n] <= containerWidth) return [];
  // dp[i]: least cost of laying out units i..n-1.
  const dp = new Float64Array(n + 1).fill(Number.POSITIVE_INFINITY);
  const nextBreak = new Int32Array(n + 1).fill(-1);
  dp[n] = 0;
  const penaltyCJK = (containerWidth * CJK_BREAK_PENALTY_RATIO) ** 2;
  const penaltyNormal = (containerWidth * NORMAL_BREAK_PENALTY_RATIO) ** 2;
  for (let i = n - 1; i >= 0; i--) {
    for (let j = i + 1; j <= n; j++) {
      const w = prefixWidth[j] - prefixWidth[i];
      let lineCost: number;
      if (w > containerWidth) {
        if (j !== i + 1) continue;
        lineCost = (w - containerWidth) ** 2 * OVERFLOW_PENALTY_MULTIPLIER;
      } else lineCost = (containerWidth - w) ** 2;
      let breakPenalty = 0;
      if (j < n) {
        const prev = units[j - 1];
        if (PUNCTUATION_REGEX.test(prev.text)) {
          breakPenalty = -((containerWidth * PUNCTUATION_BREAK_REWARD_RATIO) ** 2);
        } else if (prev.isSpace) breakPenalty = -((containerWidth * SPACE_BREAK_REWARD_RATIO) ** 2);
        else if (cjkBoundaries.has(charOffsets[j])) breakPenalty = penaltyCJK;
        else breakPenalty = penaltyNormal;
      }
      const total = lineCost + breakPenalty + dp[j];
      if (total < dp[i]) {
        dp[i] = total;
        nextBreak[i] = j;
      }
    }
  }
  const breaks: number[] = [];
  for (let at = nextBreak[0]; at > 0 && at < n; at = nextBreak[at]) breaks.push(at);
  return breaks;
}
