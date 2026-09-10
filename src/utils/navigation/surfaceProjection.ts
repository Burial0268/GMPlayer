import type { SurfaceRect } from "./sources";

export interface CoverShape {
  rect: SurfaceRect;
  radius: number;
}

/** The image owns its corners, so its transform scales the radius with its pixels. */
export function sharedImageFrames(from: CoverShape, to: CoverShape, origin: SurfaceRect) {
  const transform = ({ rect }: CoverShape) =>
    `translate3d(${rect.x - origin.x}px, ${rect.y - origin.y}px, 0) scale(${rect.width / to.rect.width}, ${rect.height / to.rect.height})`;
  const radius = (shape: CoverShape) =>
    `${(shape.radius / Math.min(shape.rect.width, shape.rect.height)) * 100}%`;
  return {
    transform: [transform(from), transform(to)],
    borderRadius: [radius(from), radius(to)],
  };
}

export const cropClip = (crop: SurfaceRect, bounds: SurfaceRect, radius: number) =>
  `inset(${crop.y - bounds.y}px ${bounds.x + bounds.width - crop.x - crop.width}px ${bounds.y + bounds.height - crop.y - crop.height}px ${crop.x - bounds.x}px round ${radius}px)`;

/** Roundness follows the image's scale; a circle is 0.5 at every size. */
export function projectedCoverFrames(from: CoverShape, to: CoverShape): CoverShape[] {
  const roundness = (shape: CoverShape) =>
    shape.radius / Math.min(shape.rect.width, shape.rect.height);
  const start = roundness(from);
  const end = roundness(to);
  const mix = (a: number, b: number, p: number) => a + (b - a) * p;
  return Array.from({ length: 129 }, (_, index) => {
    const p = index / 128;
    const rect = {
      x: mix(from.rect.x, to.rect.x, p),
      y: mix(from.rect.y, to.rect.y, p),
      width: mix(from.rect.width, to.rect.width, p),
      height: mix(from.rect.height, to.rect.height, p),
    };
    return { rect, radius: mix(start, end, p) * Math.min(rect.width, rect.height) };
  });
}

export interface SurfaceScale {
  x: number;
  y: number;
  radiusX: number;
  radiusY: number;
}

/** Correct the radius throughout a projection, not just at its two endpoints. */
export function projectedSurfaceClips(
  viewport: SurfaceRect,
  origin: SurfaceRect,
  from: SurfaceScale,
  to: SurfaceScale,
): string[] {
  const mix = (a: number, b: number, p: number) => a + (b - a) * p;
  const x = viewport.x - origin.x;
  const y = viewport.y - origin.y;
  // Dense native keyframes keep the correction off the JS frame loop.
  return Array.from({ length: 129 }, (_, index) => {
    const p = index / 128;
    const rx = mix(from.radiusX, to.radiusX, p) / mix(from.x, to.x, p);
    const ry = mix(from.radiusY, to.radiusY, p) / mix(from.y, to.y, p);
    return `inset(${y}px calc(100% - ${x + viewport.width}px) calc(100% - ${y + viewport.height}px) ${x}px round ${rx}px / ${ry}px)`;
  });
}
