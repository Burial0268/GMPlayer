/**
 * 把网易云歌词里不是歌词的行拿掉：署名（作词、作曲、编曲、乐手……）、版权与推广
 * 声明、标题行。署名里的创作者交给尾栏，其余直接丢掉——歌名、歌手、专辑在播放器
 * 里本来就有更显眼的位置。
 *
 * 署名有两种写法：普通的 `[00:00.000] 作词 : 周杰伦`，以及网易云的元数据行
 * `{"t":0,"c":[{"tx":"作词: "},{"tx":"周杰伦"}]}`。后者在 lrc 和 yrc 里都会出现，
 * 不是歌词：AMLL 的 parseYrc 会丢掉它，parseLrc 却会把排在最后一个时间戳之后的
 * 几行连同 JSON 原文一起挂到最后一行上。
 *
 * 除元数据行外，只在两种位置下手：从开头、结尾往里剥，遇到第一句歌词为止；以及
 * 连成一段的署名块（至少三行，其中至少两行角色认得出）。正文里零星带冒号、带
 * "版权"字样的句子不碰。
 */

import type { LyricCredits, LyricSongInfo } from "../types";
import { CREDIT_ROLE_RULES } from "./creditRoles";

const CREDIT_LINE = /^(.+?)\s*[:：]\s*(.*)$/;
/** 没有冒号的制作人标记：`Prod.WavyArty2`、`Produced by X` */
const PRODUCER_TAG = /^prod(?:uced)?(?:\.\s*|\s+)(?:by\s+)?([^:：]+)$/i;
/** 上游 exact 模式的默认阈值：角色里至少一半的字被规则盖住 */
const ROLE_COVERAGE = 0.5;
const NAME_SEPARATOR = /[/／、,，]/;
/** 名字后面的厂牌：`沈谕@SoundJoin` */
const NAME_AFFILIATION = /@.*$/;
const BLOCK_MIN_LINES = 3;
const BLOCK_MIN_KNOWN = 2;
/** 行首的时间标签：LRC 的 [mm:ss.xx]、YRC/QRC 的 [start,duration]、[ti:] 一类元数据 */
const LEADING_TAGS = /^\s*(?:\[[^\]]*\])+/;
/** 逐字时间：YRC 的 (t,d,0)、QRC 的 (t,d)、ESLrc 的 [mm:ss.xx] */
const WORD_TIMINGS = /\(\d+,\d+(?:,\d+)?\)|\[\d+:\d+(?:[.:]\d+)?\]/g;
const GLOBAL_RULES = CREDIT_ROLE_RULES.map(
  (rule) => new RegExp(rule.source, rule.flags.includes("g") ? rule.flags : `${rule.flags}g`),
);

// 都是从实际歌词里看到的写法。"特别""计划""专辑""来自"单独出现在歌词里很常见，
// 所以只认成句的说法
const NOTICE_PATTERNS = [
  /未经.{0,16}(?:许可|授权|同意)/,
  /(?:严禁|禁止|不得).{0,20}(?:翻唱|翻录|使用|商用|转载|传播|发布)/,
  /违者必究|版权所有|all rights reserved|unauthori[sz]ed/i,
  /^本(?:歌曲|作品|曲|单曲)/,
  /现金激励|流量扶持/,
  /网易云音乐.{0,4}(?:企划|计划)/,
  // 翻唱、改编版开头注明原曲：`《Never meant to hurt you》官方中文版`
  /^《[^》]+》.{0,6}版$/,
];
const TITLE_MARKS = /^[《“"「『](.+)[》”"」』]$/;
const BRACKETED = /[（(【[].*?[)）】\]]/g;

// 作词 / 作曲那一类，即 AMLL 所说的 songwriters；编曲、制作人不算
const SONGWRITER_ROLES_ZH = new Set([
  "作词",
  "填词",
  "中文词",
  "英文词",
  "词",
  "作曲",
  "谱曲",
  "曲",
  "词曲",
  "词曲创作",
  "作词作曲",
  "作曲作词",
]);
const SONGWRITER_ROLES_EN =
  /^(?:lyrics?(?: by)?|lyricist|words(?: by)?|written by|writer|songwriters?|composers?|composed by|composition|music(?: by)?)$/;

type Credit = { role: string; names: string[] };
type ParsedCredit = Credit & { known: boolean };
/** credit：角色认得出；shaped：长得像 `角色：名字`，但角色不在规则表里 */
type LineKind = "json" | "blank" | "credit" | "shaped" | "notice" | "lyric";

/** 规则命中的字数（不计空白）达到角色长度的一半，才当它是署名角色 */
function isCreditRole(role: string): boolean {
  const target = role.toLowerCase();
  const needed = Math.ceil(target.replace(/\s/g, "").length * ROLE_COVERAGE);
  if (!needed) return false;
  const covered = new Uint8Array(target.length);
  let count = 0;
  for (const rule of GLOBAL_RULES) {
    for (const match of target.matchAll(rule)) {
      const start = match.index ?? 0;
      const end = start + match[0].length;
      for (let i = start; i < end; i++) {
        if (covered[i] || /\s/.test(target[i])) continue;
        covered[i] = 1;
        count++;
      }
      if (count >= needed) return true;
    }
  }
  return false;
}

/** 双语角色（如 "作曲 Composer"）看开头的中文词，否则看整个英文角色 */
function isSongwriterRole(role: string): boolean {
  const leading = role.match(/^[\u4e00-\u9fff]+/)?.[0];
  if (leading) return SONGWRITER_ROLES_ZH.has(leading);
  return SONGWRITER_ROLES_EN.test(role.toLowerCase().replace(/\s+/g, " ").trim());
}

function splitNames(text: string): string[] {
  return text
    .split(NAME_SEPARATOR)
    .map((name) => name.replace(NAME_AFFILIATION, "").trim())
    .filter(Boolean);
}

function parseCredit(text: string): ParsedCredit | null {
  const trimmed = text.trim();
  const producer = trimmed.match(PRODUCER_TAG);
  if (producer) return { role: "Prod", names: splitNames(producer[1]), known: true };
  const match = trimmed.match(CREDIT_LINE);
  if (!match) return null;
  const names = splitNames(match[2]);
  const known = isCreditRole(match[1]);
  // 角色认不出又没有名字的，多半是 "林达浪：" 这样的分唱标记
  if (!known && !names.length) return null;
  return { role: match[1].trim(), names, known };
}

/** 网易云元数据行的文字；不是这种行返回 null */
function jsonLineText(line: string): string | null {
  if (!line.startsWith("{")) return null;
  try {
    const segments = JSON.parse(line)?.c;
    if (!Array.isArray(segments)) return null;
    return segments.map((segment) => segment?.tx ?? "").join("");
  } catch {
    return null;
  }
}

function lineKind(json: string | null, content: string, credit: ParsedCredit | null): LineKind {
  if (json !== null) return "json";
  if (!content.trim()) return "blank";
  if (credit) return credit.known ? "credit" : "shaped";
  if (NOTICE_PATTERNS.some((pattern) => pattern.test(content))) return "notice";
  return "lyric";
}

/** 连成一段的署名块里的行号；空行不打断一段 */
function creditBlockLines(kinds: readonly LineKind[]): Set<number> {
  const result = new Set<number>();
  let run: number[] = [];
  let known = 0;
  const flush = () => {
    if (run.length >= BLOCK_MIN_LINES && known >= BLOCK_MIN_KNOWN) {
      run.forEach((i) => result.add(i));
    }
    run = [];
    known = 0;
  };
  kinds.forEach((kind, i) => {
    if (kind === "blank") return;
    if (kind !== "credit" && kind !== "shaped" && kind !== "json") return flush();
    run.push(i);
    if (kind !== "shaped") known++;
  });
  flush();
  return result;
}

const normalize = (text: string) => text.toLowerCase().replace(/[\p{P}\p{S}\s]/gu, "");

/**
 * 标题行的判定。歌名常常就是第一句歌词（《有你爱就明媚》《天还是阴》），所以
 * 光是等于歌名还不够：要么带书名号、引号，要么写成 `歌名-歌手`，要么紧跟着的
 * 是歌手行、署名或声明。
 */
function titleMatcher(song: LyricSongInfo | undefined) {
  const name = song?.name ? normalize(song.name) : "";
  if (!name) return null;
  const base = normalize(song!.name!.replace(BRACKETED, "")) || name;
  const artists = (song!.artists ?? []).map(normalize).filter(Boolean);
  const allArtists = artists.join("");
  const credited = (text: string) =>
    [allArtists, ...artists].some(
      (artist) =>
        artist && [name, base].some((title) => text === title + artist || text === artist + title),
    );
  return {
    isArtist: (text: string) => {
      const value = normalize(text);
      return !!value && (value === allArtists || artists.includes(value));
    },
    /** strong：不看下一行就能认定；weak：要下一行也不是歌词 */
    title: (text: string): "strong" | "weak" | null => {
      const marked = text.trim().match(TITLE_MARKS)?.[1];
      const value = normalize(marked ?? text);
      if (!value) return null;
      if (credited(value)) return "strong";
      if (!value.startsWith(base)) return null;
      return marked ? "strong" : "weak";
    },
  };
}

/** 去掉一份歌词文本里不是歌词的行，返回剩下的文本和认出来的署名 */
export function splitNonLyricLines(
  text: string,
  song?: LyricSongInfo,
): { text: string; credits: Credit[] } {
  const lines = text.split("\n");
  const kinds: LineKind[] = [];
  const contents: string[] = [];
  const parsed: (ParsedCredit | null)[] = [];
  for (const line of lines) {
    const json = jsonLineText(line.trim());
    const content = json ?? line.replace(LEADING_TAGS, "").replace(WORD_TIMINGS, "");
    const credit = content.trim() ? parseCredit(content) : null;
    contents.push(content);
    parsed.push(credit);
    kinds.push(lineKind(json, content, credit));
  }

  const drop = lines.map((_, i) => kinds[i] === "json");
  creditBlockLines(kinds).forEach((i) => (drop[i] = true));
  const peelable = (i: number) => drop[i] || kinds[i] === "credit" || kinds[i] === "notice";
  const nextText = (i: number) => {
    for (let j = i + 1; j < lines.length; j++) if (kinds[j] !== "blank") return j;
    return -1;
  };

  // 从开头往里剥；标题只认一次，紧跟在它后面的歌手行一起去掉
  const titles = titleMatcher(song);
  let titleSeen = false;
  for (let i = 0; i < lines.length; i++) {
    if (kinds[i] === "blank") continue;
    if (peelable(i)) {
      drop[i] = true;
      continue;
    }
    const title = !titleSeen && kinds[i] === "lyric" ? titles?.title(contents[i]) : null;
    if (!title) break;
    const next = nextText(i);
    const artistLine = next >= 0 && kinds[next] === "lyric" && titles!.isArtist(contents[next]);
    if (title === "weak" && !artistLine && !(next >= 0 && peelable(next))) break;
    titleSeen = true;
    drop[i] = true;
    if (artistLine) {
      drop[next] = true;
      i = next;
    }
  }
  // 从结尾往里剥
  for (let i = lines.length - 1; i >= 0; i--) {
    if (kinds[i] === "blank") continue;
    if (!peelable(i)) break;
    drop[i] = true;
  }

  const credits: Credit[] = [];
  const kept: string[] = [];
  lines.forEach((line, i) => {
    if (!drop[i]) {
      kept.push(line);
      return;
    }
    const credit = parsed[i];
    if (credit?.names.length) credits.push({ role: credit.role, names: credit.names });
  });
  return { text: kept.join("\n"), credits };
}

const LYRIC_FIELDS = ["lrc", "tlyric", "romalrc", "yrc", "ytlrc", "yromalrc"] as const;

/**
 * 对一首歌的各份歌词文本去掉非歌词行（原地改传入的对象），返回尾栏要显示的创作者。
 * 翻译、音译里的署名行也去掉，只是不取它们的名字：那是原文署名的译文。
 */
export function stripNonLyricLines(
  texts: Partial<Record<(typeof LYRIC_FIELDS)[number], string | null>>,
  song?: LyricSongInfo,
): LyricCredits | undefined {
  const songwriters = new Set<string>();
  for (const field of LYRIC_FIELDS) {
    const text = texts[field];
    if (!text) continue;
    const { text: rest, credits } = splitNonLyricLines(text, song);
    texts[field] = rest;
    if (field !== "lrc" && field !== "yrc") continue;
    for (const credit of credits) {
      if (isSongwriterRole(credit.role)) credit.names.forEach((name) => songwriters.add(name));
    }
  }
  return songwriters.size ? { authors: [], songwriters: [...songwriters] } : undefined;
}
