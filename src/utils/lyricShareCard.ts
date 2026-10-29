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

const WIDTH = 1080;
const PAD = 88;
const COVER = 144;
const MAIN_SIZE = 60;
const SUB_SIZE = 38;
const GROUP_GAP = 40;
const SECTION_GAP = 80;
const FOOTER = 44;

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
  ctx.font = font(700, MAIN_SIZE);
  const groups = input.lines.map((line) => {
    ctx.font = font(700, MAIN_SIZE);
    const main = wrap(ctx, line.text, textWidth);
    ctx.font = font(500, SUB_SIZE);
    const subs = line.subs.flatMap((sub) => wrap(ctx, sub, textWidth));
    return { main, subs };
  });
  const mainLH = MAIN_SIZE * 1.3;
  const subLH = SUB_SIZE * 1.4;
  const lyricsHeight =
    groups.reduce((h, g) => h + g.main.length * mainLH + g.subs.length * subLH, 0) +
    GROUP_GAP * Math.max(0, groups.length - 1);
  const height = PAD + COVER + SECTION_GAP + lyricsHeight + SECTION_GAP + FOOTER + PAD;
  canvas.width = WIDTH;
  canvas.height = Math.ceil(height);

  // Background: theme colour underneath, so the blurred cover's soft edges
  // fade into the song's own colour instead of transparency.
  const theme = `rgb(${input.themeRgb})`;
  ctx.fillStyle = theme;
  ctx.fillRect(0, 0, WIDTH, canvas.height);
  if (cover) {
    const bleed = 160;
    const side = Math.max(WIDTH, canvas.height) + bleed * 2;
    ctx.filter = "blur(64px) saturate(1.4)";
    ctx.drawImage(cover, (WIDTH - side) / 2, (canvas.height - side) / 2, side, side);
    ctx.filter = "none";
  } else {
    const shade = ctx.createLinearGradient(0, 0, 0, canvas.height);
    shade.addColorStop(0, "rgb(255 255 255 / 0.12)");
    shade.addColorStop(1, "rgb(0 0 0 / 0.25)");
    ctx.fillStyle = shade;
    ctx.fillRect(0, 0, WIDTH, canvas.height);
  }
  ctx.fillStyle = "rgb(0 0 0 / 0.28)";
  ctx.fillRect(0, 0, WIDTH, canvas.height);

  // Header: cover thumbnail, title, artist.
  let y = PAD;
  if (cover) {
    ctx.save();
    roundRectPath(ctx, PAD, y, COVER, 16);
    ctx.clip();
    ctx.drawImage(cover, PAD, y, COVER, COVER);
    ctx.restore();
  }
  const headerX = cover ? PAD + COVER + 32 : PAD;
  const headerWidth = WIDTH - PAD - headerX;
  ctx.textBaseline = "middle";
  ctx.fillStyle = "#fff";
  ctx.font = font(700, 44);
  ctx.fillText(
    wrap(ctx, input.title, headerWidth)[0] ?? "",
    headerX,
    y + COVER / 2 - 26,
    headerWidth,
  );
  ctx.fillStyle = "rgb(255 255 255 / 0.7)";
  ctx.font = font(500, 34);
  ctx.fillText(
    wrap(ctx, input.artist, headerWidth)[0] ?? "",
    headerX,
    y + COVER / 2 + 28,
    headerWidth,
  );

  // Lyrics.
  y += COVER + SECTION_GAP;
  ctx.textBaseline = "top";
  groups.forEach((group, i) => {
    if (i > 0) y += GROUP_GAP;
    ctx.fillStyle = "#fff";
    ctx.font = font(700, MAIN_SIZE);
    for (const row of group.main) {
      ctx.fillText(row, PAD, y + (mainLH - MAIN_SIZE) / 2);
      y += mainLH;
    }
    ctx.fillStyle = "rgb(255 255 255 / 0.7)";
    ctx.font = font(500, SUB_SIZE);
    for (const row of group.subs) {
      ctx.fillText(row, PAD, y + (subLH - SUB_SIZE) / 2);
      y += subLH;
    }
  });

  // Footer: app mark.
  y += SECTION_GAP;
  ctx.globalAlpha = 0.75;
  if (logo) ctx.drawImage(logo, PAD, y, FOOTER, FOOTER);
  ctx.fillStyle = "#fff";
  ctx.textBaseline = "middle";
  ctx.font = font(600, 30);
  ctx.fillText(input.brand, logo ? PAD + FOOTER + 16 : PAD, y + FOOTER / 2);
  ctx.globalAlpha = 1;

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
    document.fonts.load(`700 ${MAIN_SIZE}px ${input.fontFamily}`),
    document.fonts.load(`500 ${SUB_SIZE}px ${input.fontFamily}`),
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
