const DESKTOP_APP_VERSION = "3.1.35";
const DESKTOP_APP_VERSION_CODE = "205293";
const DESKTOP_NSM = "1.0.0";
const DESKTOP_CHANNEL = "netease";
const DESKTOP_STORAGE_KEY = "neteaseDesktopClientContext";
const SESSION_CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

interface DesktopClientContext {
  cid: string;
  nnid: string;
  nuid: string;
  deviceId: string;
  mode: string;
  osver: string;
}

const randomString = (length: number, chars = SESSION_CHARS): string => {
  let result = "";
  const cryptoObject = globalThis.crypto;
  const values = new Uint32Array(length);

  if (cryptoObject?.getRandomValues) {
    cryptoObject.getRandomValues(values);
    for (let i = 0; i < length; i++) {
      result += chars[values[i] % chars.length];
    }
    return result;
  }

  for (let i = 0; i < length; i++) {
    result += chars[Math.floor(Math.random() * chars.length)];
  }
  return result;
};

const parseCookie = (cookie: string | null | undefined): Record<string, string> => {
  const result: Record<string, string> = {};
  if (!cookie) return result;

  for (const part of cookie.split(";")) {
    const trimmed = part.trim();
    if (!trimmed) continue;

    const eqIndex = trimmed.indexOf("=");
    if (eqIndex <= 0) continue;

    const key = trimmed.slice(0, eqIndex).trim();
    const value = trimmed.slice(eqIndex + 1).trim();
    if (key) result[key] = value;
  }

  return result;
};

const stringifyCookie = (cookie: Record<string, string>): string => {
  return Object.entries(cookie)
    .filter(([key]) => key)
    .map(([key, value]) => `${key}=${value}`)
    .join("; ");
};

const getDefaultOsVersion = (): string => {
  const platformVersion = import.meta.env.TAURI_PLATFORM_VERSION;
  if (platformVersion) return `Microsoft-Windows-${platformVersion}-64bit`;
  return "Microsoft-Windows-11-Professional-build-22631-64bit";
};

const createDesktopClientContext = (): DesktopClientContext => {
  const timestamp = Date.now();

  return {
    cid: `lqdloz.${timestamp}.01.0`,
    nnid: `${timestamp},${randomString(32)}`,
    nuid: randomString(32),
    deviceId: `${randomString(8)}-${randomString(4)}-${randomString(4)}-${randomString(4)}-${randomString(12)}`,
    mode: "GMPlayer",
    osver: getDefaultOsVersion(),
  };
};

const getDesktopClientContext = (): DesktopClientContext => {
  if (typeof localStorage === "undefined") return createDesktopClientContext();

  const saved = localStorage.getItem(DESKTOP_STORAGE_KEY);
  if (saved) {
    try {
      const parsed = JSON.parse(saved) as Partial<DesktopClientContext>;
      if (parsed.cid && parsed.nnid && parsed.nuid && parsed.deviceId) {
        return {
          cid: parsed.cid,
          nnid: parsed.nnid,
          nuid: parsed.nuid,
          deviceId: parsed.deviceId,
          mode: parsed.mode || "GMPlayer",
          osver: parsed.osver || getDefaultOsVersion(),
        };
      }
    } catch {
      localStorage.removeItem(DESKTOP_STORAGE_KEY);
    }
  }

  const context = createDesktopClientContext();
  localStorage.setItem(DESKTOP_STORAGE_KEY, JSON.stringify(context));
  return context;
};

export const createNeteasePlaybackSessionId = (): string => randomString(12);

export const buildNeteaseDesktopUserAgent = (): string => {
  return `Mozilla/5.0 (Windows NT 10.0; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Safari/537.36 Chrome/91.0.4472.164 NeteaseMusicDesktop/${DESKTOP_APP_VERSION}`;
};

export const buildNeteaseDesktopCookie = (rawCookie: string | null | undefined): string => {
  const cookie = parseCookie(rawCookie);
  const context = getDesktopClientContext();

  const fallback: Record<string, string> = {
    "JSESSIONID-WYYY": "",
    MUSIC_U: cookie.MUSIC_U ?? "",
    NMTID: cookie.NMTID ?? "",
    WEVNSM: DESKTOP_NSM,
    WNMCID: context.cid,
    __csrf: cookie.__csrf ?? "",
    _iuqxldmzr_: "33",
    _ntes_nnid: context.nnid,
    _ntes_nuid: context.nuid,
    appver: `${DESKTOP_APP_VERSION}.${DESKTOP_APP_VERSION_CODE}`,
    channel: DESKTOP_CHANNEL,
    clientSign: "",
    deviceId: context.deviceId,
    mode: context.mode,
    ntes_kaola_ad: "1",
    os: "pc",
    osver: context.osver,
  };

  return stringifyCookie({ ...fallback, ...cookie });
};
