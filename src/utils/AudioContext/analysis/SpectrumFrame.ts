export type SpectrumFrame = ArrayLike<number>;

const EMPTY_SPECTRUM: readonly number[] = Object.freeze([]);

let currentFrame: SpectrumFrame = EMPTY_SPECTRUM;
let version = 0;

export const setSpectrumFrame = (frame: SpectrumFrame | null | undefined): void => {
  currentFrame = frame && frame.length > 0 ? frame : EMPTY_SPECTRUM;
  version++;
};

export const clearSpectrumFrame = (): void => {
  currentFrame = EMPTY_SPECTRUM;
  version++;
};

export const getSpectrumFrame = (): SpectrumFrame => currentFrame;

export const getSpectrumVersion = (): number => version;
