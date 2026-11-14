export type SpectrumFrame = ArrayLike<number>;

const EMPTY_SPECTRUM: readonly number[] = Object.freeze([]);

let currentFrame: SpectrumFrame = EMPTY_SPECTRUM;
let currentScale = 1;
let version = 0;

export const setSpectrumFrame = (frame: SpectrumFrame | null | undefined, scale?: number): void => {
  currentFrame = frame && frame.length > 0 ? frame : EMPTY_SPECTRUM;
  if (typeof scale === "number" && Number.isFinite(scale)) {
    currentScale = scale;
  }
  version++;
};

export const clearSpectrumFrame = (): void => {
  currentFrame = EMPTY_SPECTRUM;
  currentScale = 1;
  version++;
};

export const getSpectrumFrame = (): SpectrumFrame => currentFrame;

export const getSpectrumScale = (): number => currentScale;

export const getSpectrumVersion = (): number => version;
