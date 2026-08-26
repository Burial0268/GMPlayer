/**
 * NativeResolverConfigSync — hands the Rust backend the endpoints, credentials
 * and transport choice it needs to resolve playback URLs itself.
 *
 * The backend does NOT reimplement the Netease API. Depending on the selected
 * transport it either calls the deployed NeteaseCloudMusicApi over plain HTTP,
 * or the in-process protocol layer (`crates/ncm-core`) that the UI is already
 * using — the same code either way, never a second implementation. This module
 * is the single place that decides which endpoints, which credentials and which
 * transport that is, so the two runtimes can never drift apart.
 *
 * What is duplicated in Rust is only the five-rule fallback policy from
 * `resolveSongUrl.ts` (level, VIP pre-check, trial detection, UNM fallback,
 * kuwo proxy) — see `player/source_resolver.rs`, which unit-tests the same
 * cases.
 *
 * Secrets: the cookie is sent over the local Tauri IPC only. The backend never
 * logs or persists it.
 */

import { isTauri } from "@/utils/tauri/core/runtime";
import type { NativeResolverConfig } from "@/utils/tauri/audio/protocol";
import { NativeRustSound } from "@/utils/tauri/audio/nativeRustSound";
import useSettingDataStore from "@/store/settingData";
import { userStore } from "@/store";
import { getNcmTransport } from "@/utils/request";

const IS_DEV = import.meta.env?.DEV ?? false;

let lastSerialized = "";

/**
 * Resolve the NCM API base the same way `utils/request.ts` does.
 *
 * In dev the frontend talks to Vite's same-origin proxy (`/api/ncm`), which
 * Rust cannot use — it has no dev server. Fall back to the configured upstream
 * so native resolution works in `pnpm dev` too.
 */
const resolveNcmBase = (): string | null => {
  const configured = import.meta.env.VITE_MUSIC_API as string | undefined;
  return configured?.trim() || null;
};

const resolveUnmBase = (): string | null => {
  const configured = import.meta.env.VITE_UNM_API as string | undefined;
  return configured?.trim() || null;
};

/** Netease id of the signed-in account, or `null` when signed out. */
const resolveUserId = (user: ReturnType<typeof userStore>): string | null => {
  if (!user.userLogin) return null;
  const profile = user.userData as { userId?: number | string; id?: number | string } | undefined;
  const raw = profile?.userId ?? profile?.id;
  const id = raw === undefined || raw === null ? "" : String(raw).trim();
  return id && id !== "0" ? id : null;
};

/**
 * Push the current resolver config to the backend. Idempotent — skips IPC when
 * nothing changed. Call on startup, login/logout and quality-setting changes.
 */
export const syncNativeResolverConfig = (options: { force?: boolean } = {}): void => {
  if (!isTauri()) return;
  const sound = window.$player;
  if (!(sound instanceof NativeRustSound) || sound.isDestroyed()) return;

  const setting = useSettingDataStore();
  const user = userStore();

  const unmBaseUrl = resolveUnmBase();
  const config: NativeResolverConfig = {
    ncmBaseUrl: resolveNcmBase(),
    unmBaseUrl,
    unmEnabled: Boolean(unmBaseUrl) && Boolean(setting.useUnmServer),
    cookie: user.cookie ?? null,
    // Lets the backend fetch `/likelist` for itself, which is what makes the
    // notification's heart correct while the WebView is destroyed.
    //
    // `userId` first: that is the field the profile actually carries (see
    // `userData.userId` in the user store). `id` is kept as a fallback because
    // some responses use it, and reading only `id` silently yielded `null` —
    // which cost nothing visible except that the like list never loaded.
    userId: resolveUserId(user),
    level: setting.songLevel || "exhigh",
    // Playback resolution follows the same transport as the UI. Splitting them
    // would mean two sessions and two source IPs, with only one half getting
    // the benefit — and the native planner keeps resolving while the WebView is
    // frozen, so it has to be told rather than asked.
    useLocalNcm: getNcmTransport() === "local",
  };

  const serialized = JSON.stringify(config);
  if (!options.force && serialized === lastSerialized) return;
  lastSerialized = serialized;

  sound.setNativeResolverConfig(config);
  if (IS_DEV) {
    // Never log the cookie itself — only whether one is present.
    console.log(
      `[NativeResolver] config synced: ncm=${Boolean(config.ncmBaseUrl)}, ` +
        `local=${config.useLocalNcm}, unm=${config.unmEnabled}, ` +
        `level=${config.level}, auth=${Boolean(config.cookie)}, ` +
        `uid=${Boolean(config.userId)}`,
    );
  }
};

/** Force a re-push on the next call (e.g. after the player is recreated). */
export const invalidateNativeResolverConfig = (): void => {
  lastSerialized = "";
};
