/**
 * Draws the shareable lyric card onto a canvas: blurred cover (theme colour
 * when there is none), song header, the selected lines with their translation
 * and romanisation, and the app mark. Hand-drawn rather than rasterising DOM —
 * the lyric view is AMLL's springs and masks, which no DOM-to-image library
 * reproduces, and the card is a few rectangles and text runs.
 */

export interface LyricShareLine {
  text: string;
  /** Translation / romanisation, already filtered by the display settings. */
  subs: string[];
}

export interface LyricCardInput {
  lines: LyricShareLine[];
  title: string;
  artist: string;
  coverSrc: string;
  /** `"r, g, b"`, the shape `siteStore.songPicColor` holds. */
  themeRgb: string;
  fontFamily: string;
  brand: string;
  logoSrc: string;
}

import { srgbToOkLab } from "@applemusic-like-lyrics/core";

/**
 * The theme colour pulled dark enough for white text: lightness and chroma
 * capped in OKLCH, hue kept, so a pale or neon cover still reads as its own
 * colour but never as a backdrop white text disappears into. Shared by the
 * selection sheet and the card so both sit on the same tone.
 */
export function shareTone(themeRgb: string, alpha = 1): string {
  const [r, g, b] = themeRgb.split(",").map((v) => Number(v) / 255);
  const [l, a, bb] = srgbToOkLab([r, g, b]);
  const lightness = Math.min(l, 0.4);
  const chroma = Math.min(Math.hypot(a, bb), 0.12);
  const hue = (Math.atan2(bb, a) * 180) / Math.PI;
  return `oklch(${lightness.toFixed(3)} ${chroma.toFixed(3)} ${hue.toFixed(1)} / ${alpha})`;
}

const WIDTH = 1080;
// 4:5 at the least, the portrait shape share targets crop to least.
const MIN_HEIGHT = 1350;
const PAD = 96;
const MAIN_SIZE = 68;
const SUB_SIZE = 40;
const GROUP_GAP = 44;
const SECTION_GAP = 96;
const COVER = 120;
const LOGO = 40;
/** Veil opacity at the top and bottom edge of the card. */
export const VEIL = [0.62, 0.8] as const;

const loadImage = (src: string): Promise<HTMLImageElement | null> =>
  new Promise((resolve) => {
    const image = new Image();
    image.crossOrigin = "anonymous";
    image.onload = () => resolve(image);
    image.onerror = () => resolve(null);
    image.src = src;
  });

// CJK characters break anywhere; everything else breaks at whitespace.
const TOKEN = /[぀-ヿ㐀-鿿가-힯＀-￯]|[^\s぀-ヿ㐀-鿿가-힯＀-￯]+|\s+/g;

function wrap(ctx: CanvasRenderingContext2D, text: string, width: number): string[] {
  const rows: string[] = [];
  let row = "";
  for (const token of text.match(TOKEN) ?? []) {
    const next = row + token;
    if (ctx.measureText(next).width <= width || !row.trim()) {
      row = next;
      // A single token wider than the card (a long Latin word): split by char.
      while (ctx.measureText(row).width > width && row.length > 1) {
        let cut = row.length - 1;
        while (cut > 1 && ctx.measureText(row.slice(0, cut)).width > width) cut--;
        rows.push(row.slice(0, cut));
        row = row.slice(cut);
      }
    } else {
      rows.push(row.trimEnd());
      row = token.trimStart();
    }
  }
  if (row.trim()) rows.push(row.trimEnd());
  return rows;
}

function ellipsize(ctx: CanvasRenderingContext2D, text: string, width: number): string {
  if (ctx.measureText(text).width <= width) return text;
  let cut = text;
  while (cut && ctx.measureText(`${cut}…`).width > width) cut = cut.slice(0, -1);
  return `${cut.trimEnd()}…`;
}

function roundRectPath(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  size: number,
  r: number,
) {
  ctx.beginPath();
  ctx.roundRect(x, y, size, size, r);
}

function draw(
  input: LyricCardInput,
  cover: HTMLImageElement | null,
  logo: HTMLImageElement | null,
) {
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d")!;
  const font = (weight: number, size: number) =>
    `${weight} ${size}px ${input.fontFamily}, system-ui, sans-serif`;
  const textWidth = WIDTH - PAD * 2;

  // Measure first: the height depends on how the lines wrap.
  const groups = input.lines.map((line) => {
    ctx.font = font(800, MAIN_SIZE);
    const main = wrap(ctx, line.text, textWidth);
    ctx.font = font(600, SUB_SIZE);
    const subs = line.subs.flatMap((sub) => wrap(ctx, sub, textWidth));
    return { main, subs };
  });
  const mainLH = MAIN_SIZE * 1.25;
  const subLH = SUB_SIZE * 1.4;
  const lyricsHeight =
    groups.reduce((h, g) => h + g.main.length * mainLH + g.subs.length * subLH, 0) +
    GROUP_GAP * Math.max(0, groups.length - 1);
  const height = Math.ceil(Math.max(MIN_HEIGHT, PAD + lyricsHeight + SECTION_GAP + COVER + PAD));
  canvas.width = WIDTH;
  canvas.height = height;

  // Background: the darkened tone underneath, so the blurred cover's soft
  // edges fade into it, then a veil of the same tone over the cover. A black
  // veil would grey a pale cover out; the tone keeps its hue.
  ctx.fillStyle = shareTone(input.themeRgb);
  ctx.fillRect(0, 0, WIDTH, height);
  if (cover) {
    const side = Math.max(WIDTH, height) * 1.3;
    ctx.filter = "blur(72px) saturate(1.3) brightness(0.75)";
    ctx.drawImage(cover, (WIDTH - side) / 2, (height - side) / 2, side, side);
    ctx.filter = "none";
  }
  const shade = ctx.createLinearGradient(0, 0, 0, height);
  shade.addColorStop(0, shareTone(input.themeRgb, VEIL[0]));
  shade.addColorStop(1, shareTone(input.themeRgb, VEIL[1]));
  ctx.fillStyle = shade;
  ctx.fillRect(0, 0, WIDTH, height);

  // Lyrics, centred in whatever room the footer leaves.
  const footerTop = height - PAD - COVER;
  let y = PAD + Math.max(0, (footerTop - SECTION_GAP - PAD - lyricsHeight) / 2);
  ctx.textBaseline = "top";
  groups.forEach((group, i) => {
    if (i > 0) y += GROUP_GAP;
    ctx.fillStyle = "#fff";
    ctx.font = font(800, MAIN_SIZE);
    for (const row of group.main) {
      ctx.fillText(row, PAD, y + (mainLH - MAIN_SIZE) / 2);
      y += mainLH;
    }
    ctx.fillStyle = "rgb(255 255 255 / 0.72)";
    ctx.font = font(600, SUB_SIZE);
    for (const row of group.subs) {
      ctx.fillText(row, PAD, y + (subLH - SUB_SIZE) / 2);
      y += subLH;
    }
  });

  // Footer: artwork, title and artist on the left, the app mark on the right.
  ctx.font = font(600, 28);
  const brandWidth = ctx.measureText(input.brand).width + (logo ? LOGO + 12 : 0);
  const brandX = WIDTH - PAD - brandWidth;
  const midY = footerTop + COVER / 2;
  ctx.globalAlpha = 0.8;
  ctx.textBaseline = "middle";
  if (logo) ctx.drawImage(logo, brandX, midY - LOGO / 2, LOGO, LOGO);
  ctx.fillStyle = "#fff";
  ctx.fillText(input.brand, logo ? brandX + LOGO + 12 : brandX, midY);
  ctx.globalAlpha = 1;

  if (cover) {
    ctx.save();
    ctx.shadowColor = "rgb(0 0 0 / 0.3)";
    ctx.shadowBlur = 24;
    ctx.shadowOffsetY = 8;
    roundRectPath(ctx, PAD, footerTop, COVER, 14);
    ctx.fillStyle = "#000";
    ctx.fill();
    ctx.restore();
    ctx.save();
    roundRectPath(ctx, PAD, footerTop, COVER, 14);
    ctx.clip();
    ctx.drawImage(cover, PAD, footerTop, COVER, COVER);
    ctx.restore();
  }
  const metaX = cover ? PAD + COVER + 28 : PAD;
  const metaWidth = brandX - 40 - metaX;
  ctx.fillStyle = "#fff";
  ctx.font = font(700, 38);
  ctx.fillText(ellipsize(ctx, input.title, metaWidth), metaX, midY - 24);
  ctx.fillStyle = "rgb(255 255 255 / 0.7)";
  ctx.font = font(500, 30);
  ctx.fillText(ellipsize(ctx, input.artist, metaWidth), metaX, midY + 24);

  return canvas;
}

const toPng = (canvas: HTMLCanvasElement): Promise<Blob> =>
  new Promise((resolve, reject) => {
    canvas.toBlob(
      (blob) => (blob ? resolve(blob) : reject(new Error("toBlob failed"))),
      "image/png",
    );
  });

export async function renderLyricCard(input: LyricCardInput): Promise<Blob> {
  const [cover, logo] = await Promise.all([loadImage(input.coverSrc), loadImage(input.logoSrc)]);
  await Promise.all([
    document.fonts.load(`800 ${MAIN_SIZE}px ${input.fontFamily}`),
    document.fonts.load(`600 ${SUB_SIZE}px ${input.fontFamily}`),
  ]).catch(() => undefined);
  try {
    return await toPng(draw(input, cover, logo));
  } catch (error) {
    // A cover served without CORS headers taints the canvas and export throws;
    // the card is still worth having on the theme colour alone.
    if (!(error instanceof DOMException && error.name === "SecurityError")) throw error;
    console.warn("[lyricShare] cover tainted the canvas, rendering without it", error);
    return toPng(draw(input, null, logo));
  }
}
