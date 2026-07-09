import { isMobile, isMobileDevice } from "./mobile";
import { isTauri } from "./windowManager";

export const MOBILE_TAURI_AUDIO_UI_DELAY_SECONDS = 0.45;

let cachedMobileTauri: boolean | null = null;
let mobileTauriPromise: Promise<boolean> | null = null;

async function resolveMobileTauri(): Promise<boolean> {
  const enabled = isTauri() && (await isMobile());
  cachedMobileTauri = enabled;
  return enabled;
}

export function primeMobileTauriAudioUiDelay(): void {
  if (cachedMobileTauri !== null || mobileTauriPromise) return;
  mobileTauriPromise = resolveMobileTauri().finally(() => {
    mobileTauriPromise = null;
  });
}

export function isMobileTauriAudioUiDelayEnabled(): boolean {
  if (!isTauri()) return false;

  if (isMobileDevice()) {
    cachedMobileTauri = true;
    return true;
  }

  if (cachedMobileTauri !== null) return cachedMobileTauri;
  primeMobileTauriAudioUiDelay();
  return false;
}

export function getMobileTauriAudioUiDelaySeconds(): number {
  return isMobileTauriAudioUiDelayEnabled() ? MOBILE_TAURI_AUDIO_UI_DELAY_SECONDS : 0;
}

export function applyMobileTauriAudioUiDelay(currentTime: number, duration = 0): number {
  const normalizedTime = Number.isFinite(currentTime) ? Math.max(0, currentTime) : 0;
  const delay = getMobileTauriAudioUiDelaySeconds();
  const displayTime = delay > 0 ? Math.max(0, normalizedTime - delay) : normalizedTime;

  return duration > 0 ? Math.min(displayTime, duration) : displayTime;
}
