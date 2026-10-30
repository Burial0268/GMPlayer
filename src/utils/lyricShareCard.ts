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
  /** A still of the player's fluid background; drawn instead of the blurred cover. */
  backdrop?: HTMLCanvasElement;
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
 * colour but never as a backdrop white text disappears into.
 */
function shareTone(themeRgb: string, alpha = 1): string {
  const [r, g, b] = themeRgb.split(",").map((v) => Number(v) / 255);
  const [l, a, bb] = srgbToOkLab([r, g, b]);
  const lightness = Math.min(l, 0.3);
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
/** The card's layout in its own 1080-wide units, for a live card that scales it to fit. */
export const CARD_METRICS = {
  width: WIDTH,
  pad: PAD,
  main: MAIN_SIZE,
  sub: SUB_SIZE,
  gap: GROUP_GAP,
  cover: COVER,
  logo: LOGO,
};
/** Veil opacity at the top and bottom edge of the card. */
const VEIL = [0.62, 0.8] as const;
// With the 0.3 lightness cap, a white cover still leaves white text at 6.6:1.
const BACKDROP_BRIGHTNESS = 0.4;
/** Black over the fluid still, top to bottom edge. */
const STILL_SHADE = [0.12, 0.4] as const;
// Lyrics and title sit a touch under pure white, as the player draws them.
const TEXT = "rgb(255 255 255 / 0.9)";

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

function paintBackground(
  ctx: CanvasRenderingContext2D,
  input: LyricCardInput,
  cover: HTMLImageElement | null,
  height: number,
  scale: number,
) {
  // Background: the darkened tone underneath, so the blurred cover's soft
  // edges fade into it, then a veil of the same tone over the cover. A black
  // veil would grey a pale cover out; the tone keeps its hue.
  ctx.fillStyle = shareTone(input.themeRgb);
  ctx.fillRect(0, 0, WIDTH, height);
  if (input.backdrop) {
    // The player's own background, as it looked: the player sets its white
    // lyrics straight on it, so it gets only a light shade for the footer
    // rather than the cover's darkening. Cover-fit; the mesh is soft enough to
    // scale up from render size.
    const { width: w, height: h } = input.backdrop;
    const fit = Math.max(WIDTH / w, height / h);
    ctx.drawImage(input.backdrop, (WIDTH - w * fit) / 2, (height - h * fit) / 2, w * fit, h * fit);
    const shade = ctx.createLinearGradient(0, 0, 0, height);
    shade.addColorStop(0, `rgb(0 0 0 / ${STILL_SHADE[0]})`);
    shade.addColorStop(1, `rgb(0 0 0 / ${STILL_SHADE[1]})`);
    ctx.fillStyle = shade;
    ctx.fillRect(0, 0, WIDTH, height);
  } else {
    if (cover) {
      const side = Math.max(WIDTH, height) * 1.3;
      ctx.filter = `blur(${72 * scale}px) saturate(1.3) brightness(${BACKDROP_BRIGHTNESS})`;
      ctx.drawImage(cover, (WIDTH - side) / 2, (height - side) / 2, side, side);
      ctx.filter = "none";
    }
    const veil = ctx.createLinearGradient(0, 0, 0, height);
    veil.addColorStop(0, shareTone(input.themeRgb, VEIL[0]));
    veil.addColorStop(1, shareTone(input.themeRgb, VEIL[1]));
    ctx.fillStyle = veil;
    ctx.fillRect(0, 0, WIDTH, height);
  }
}

function draw(
  input: LyricCardInput,
  cover: HTMLImageElement | null,
  logo: HTMLImageElement | null,
  canvas = document.createElement("canvas"),
  scale = 1,
) {
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
  // Laid out at 1080 wide either way; a preview rasterises fewer pixels.
  canvas.width = Math.round(WIDTH * scale);
  canvas.height = Math.round(height * scale);
  ctx.scale(scale, scale);

  paintBackground(ctx, input, cover, height, scale);

  // Lyrics, centred in whatever room the footer leaves.
  const footerTop = height - PAD - COVER;
  let y = PAD + Math.max(0, (footerTop - SECTION_GAP - PAD - lyricsHeight) / 2);
  ctx.textBaseline = "top";
  groups.forEach((group, i) => {
    if (i > 0) y += GROUP_GAP;
    ctx.fillStyle = TEXT;
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
    // Shadows, like filters, are in canvas pixels and ignore the transform.
    ctx.shadowBlur = 24 * scale;
    ctx.shadowOffsetY = 8 * scale;
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
  ctx.fillStyle = TEXT;
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

export interface CardAssets {
  cover: HTMLImageElement | null;
  logo: HTMLImageElement | null;
}

/** Everything a draw waits on; load once and draw as often as the lines change. */
export async function loadCardAssets(input: LyricCardInput): Promise<CardAssets> {
  const [cover, logo] = await Promise.all([loadImage(input.coverSrc), loadImage(input.logoSrc)]);
  await Promise.all([
    document.fonts.load(`800 ${MAIN_SIZE}px ${input.fontFamily}`),
    document.fonts.load(`600 ${SUB_SIZE}px ${input.fontFamily}`),
  ]).catch(() => undefined);
  return { cover, logo };
}

/** Draws synchronously, into `canvas` when given, so a preview can skip encoding. */
export const drawLyricCard = (
  input: LyricCardInput,
  assets: CardAssets,
  canvas?: HTMLCanvasElement,
  scale?: number,
) => draw(input, assets.cover, assets.logo, canvas, scale);

/**
 * Only the card's background, for a live card laid out in the DOM: `aspect` is
 * its height over its width, `scale` its canvas pixels per 1080-wide unit.
 */
export function drawCardBackdrop(
  input: LyricCardInput,
  assets: CardAssets,
  canvas: HTMLCanvasElement,
  aspect: number,
  scale: number,
) {
  const height = WIDTH * aspect;
  canvas.width = Math.round(WIDTH * scale);
  canvas.height = Math.round(height * scale);
  const ctx = canvas.getContext("2d")!;
  ctx.scale(scale, scale);
  paintBackground(ctx, input, assets.cover, height, scale);
}

/** The PNG of a drawn card. */
export async function encodeLyricCard(
  canvas: HTMLCanvasElement,
  input: LyricCardInput,
  assets: CardAssets,
): Promise<Blob> {
  try {
    return await toPng(canvas);
  } catch (error) {
    // A cover served without CORS headers taints the canvas and export throws;
    // the card is still worth having on the theme colour alone.
    if (!(error instanceof DOMException && error.name === "SecurityError")) throw error;
    console.warn("[lyricShare] cover tainted the canvas, rendering without it", error);
    return toPng(draw(input, null, assets.logo));
  }
}

export async function renderLyricCard(input: LyricCardInput): Promise<Blob> {
  const assets = await loadCardAssets(input);
  return encodeLyricCard(drawLyricCard(input, assets), input, assets);
}
