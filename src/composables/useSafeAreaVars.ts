const SAFE_AREA_VARS = [
  "--app-safe-area-top",
  "--app-safe-area-right",
  "--app-safe-area-bottom",
  "--app-safe-area-left",
] as const;

const CSS_ENV_FALLBACK: Record<(typeof SAFE_AREA_VARS)[number], string> = {
  "--app-safe-area-top": "env(safe-area-inset-top, 0px)",
  "--app-safe-area-right": "env(safe-area-inset-right, 0px)",
  "--app-safe-area-bottom": "env(safe-area-inset-bottom, 0px)",
  "--app-safe-area-left": "env(safe-area-inset-left, 0px)",
};

/**
 * Synchronous getter injected by MainActivity on Android.
 *
 * Returns "top,right,bottom,left" in CSS px, or "" before the first
 * WindowInsets callback has run.
 */
interface AndroidSafeAreaBridge {
  get(): string;
}

declare global {
  interface Window {
    AndroidSafeArea?: AndroidSafeAreaBridge;
  }
}

const applyEnvFallback = (root: HTMLElement): void => {
  for (const key of SAFE_AREA_VARS) {
    root.style.setProperty(key, CSS_ENV_FALLBACK[key]);
  }
};

const applyNativeInsets = (root: HTMLElement, payload: string): boolean => {
  const parts = payload.split(",");
  if (parts.length !== SAFE_AREA_VARS.length) return false;

  const values = parts.map((part) => Number.parseFloat(part));
  if (values.some((value) => !Number.isFinite(value) || value < 0)) return false;

  SAFE_AREA_VARS.forEach((key, index) => {
    root.style.setProperty(key, `${values[index]}px`);
  });
  return true;
};

/**
 * Expose CSS safe-area insets on every runtime.
 *
 * `env(safe-area-inset-*)` covers desktop browsers (where it resolves to zero),
 * mobile browsers and installed PWAs. It is NOT sufficient on Android: the
 * WebView only reports the display cutout through `env()` and never the system
 * bars, so under `enableEdgeToEdge()` the bottom gesture bar occupies real screen
 * space while `env(safe-area-inset-bottom)` stays 0. Bottom chrome then renders
 * one gesture-bar too low. MainActivity measures the true insets and both pushes
 * them here and exposes them via `window.AndroidSafeArea`; this reads that bridge
 * when present and falls back to `env()` everywhere else.
 *
 * Native values, once applied, are authoritative — MainActivity keeps pushing
 * updates on rotation, gesture/button navigation changes and multi-window resize.
 */
export function useSafeAreaVars(): void {
  if (typeof document === "undefined") return;

  const root = document.documentElement;
  applyEnvFallback(root);

  const bridge = window.AndroidSafeArea;
  if (!bridge) return;

  try {
    const payload = bridge.get();
    if (payload) applyNativeInsets(root, payload);
  } catch {
    // Bridge missing or throwing means we simply keep the env() fallback.
  }
}
