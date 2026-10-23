// 运行环境信息：应用版本、Tauri 版本、操作系统与渲染内核。
// 版本号优先取 userAgentData 高熵值（Chromium 系可拿到精确的 platformVersion，
// UA 字符串在 Windows / macOS 上已被冻结），不可用时回退到 UA 解析。
import { appInfo } from "@/utils/appInfo";
import { getDesktopEnvironment, isMobileDevice, isTauri } from "@/utils/tauri";

export type RuntimeKind = "desktop" | "mobile" | "web";

export interface RuntimeInfo {
  /** 应用版本：Tauri 下为原生包版本，Web 下为构建期注入的版本 */
  appVersion: string;
  /** 构建期注入的前端版本 */
  webVersion: string;
  /** Tauri 框架版本，非 Tauri 环境为 null */
  tauriVersion: string | null;
  /** 运行形态 */
  kind: RuntimeKind;
  /** 操作系统展示名，如 Windows 11 / macOS 14.5 / Android 14 */
  os: string;
  /** CPU 架构，如 x86_64 / arm64，未知为 null */
  arch: string | null;
  /** 渲染内核，如 Chromium 130 */
  engine: string;
}

interface HighEntropyValues {
  platform?: string;
  platformVersion?: string;
  architecture?: string;
  bitness?: string;
  fullVersionList?: { brand: string; version: string }[];
}

const majorOf = (version?: string) => Number.parseInt(version?.split(".")[0] ?? "", 10);

const readHighEntropy = async (): Promise<HighEntropyValues | null> => {
  const uaData = (navigator as any)?.userAgentData;
  if (!uaData?.getHighEntropyValues) return null;
  try {
    return await uaData.getHighEntropyValues([
      "platform",
      "platformVersion",
      "architecture",
      "bitness",
      "fullVersionList",
    ]);
  } catch {
    return null;
  }
};

/** UA 字符串兜底解析，仅在拿不到高熵值时使用 */
const osFromUserAgent = (): string => {
  const ua = navigator.userAgent ?? "";
  const android = /Android\s+([\d.]+)/.exec(ua);
  if (android) return `Android ${android[1]}`;
  const ios = /(?:iPhone|iPad|iPod).*?OS\s+([\d_]+)/.exec(ua);
  if (ios) return `iOS ${ios[1].replace(/_/g, ".")}`;
  if (/Windows NT 10\.0/.test(ua)) return "Windows 10/11";
  const windows = /Windows NT ([\d.]+)/.exec(ua);
  if (windows) return `Windows NT ${windows[1]}`;
  const mac = /Mac OS X ([\d_.]+)/.exec(ua);
  if (mac) return `macOS ${mac[1].replace(/_/g, ".")}`;
  if (/Linux/i.test(ua)) return "Linux";
  return "";
};

const formatOs = (os: string, entropy: HighEntropyValues | null): string => {
  const version = entropy?.platformVersion?.replace(/(\.0)+$/, "") ?? "";
  switch (os) {
    case "windows": {
      // Chromium 在 Win11 上报 platformVersion 主版本 >= 13，Win10 为 1-10，更早为 0
      const major = majorOf(entropy?.platformVersion);
      if (Number.isNaN(major)) return osFromUserAgent() || "Windows";
      if (major >= 13) return "Windows 11";
      if (major >= 1) return "Windows 10";
      return "Windows 8.1 或更早";
    }
    case "macos":
      return version ? `macOS ${version}` : "macOS";
    case "android":
      return version ? `Android ${version}` : osFromUserAgent() || "Android";
    case "ios":
      return version ? `iOS ${version}` : osFromUserAgent() || "iOS";
    case "linux":
      return version ? `Linux ${version}` : "Linux";
    default:
      return osFromUserAgent() || "";
  }
};

const formatArch = (entropy: HighEntropyValues | null): string | null => {
  const architecture = entropy?.architecture;
  if (!architecture) return null;
  const bitness = entropy.bitness;
  if (architecture === "x86") return bitness === "64" ? "x86_64" : "x86";
  if (architecture === "arm") return bitness === "64" ? "arm64" : "arm";
  return bitness ? `${architecture}-${bitness}` : architecture;
};

const formatEngine = (entropy: HighEntropyValues | null): string => {
  const chromium = entropy?.fullVersionList?.find((item) => item.brand === "Chromium");
  if (chromium) return `Chromium ${chromium.version.split(".")[0]}`;
  const ua = navigator.userAgent ?? "";
  const chrome = /Chrome\/([\d.]+)/.exec(ua);
  if (chrome) return `Chromium ${chrome[1].split(".")[0]}`;
  const firefox = /Firefox\/([\d.]+)/.exec(ua);
  if (firefox) return `Gecko ${firefox[1].split(".")[0]}`;
  const safari = /Version\/([\d.]+).*Safari/.exec(ua);
  if (safari) return `WebKit ${safari[1]}`;
  return "";
};

/** Tauri 原生版本号（应用版本 + 框架版本），Web 环境返回 null */
const readTauriVersions = async (): Promise<{ app: string; tauri: string } | null> => {
  if (!isTauri()) return null;
  try {
    const { getVersion, getTauriVersion } = await import("@tauri-apps/api/app");
    const [app, tauri] = await Promise.all([getVersion(), getTauriVersion()]);
    return { app, tauri };
  } catch (error) {
    console.error("Failed to read Tauri version:", error);
    return null;
  }
};

let runtimeInfoPromise: Promise<RuntimeInfo> | null = null;

/** 读取运行环境信息，结果在单个窗口生命周期内缓存 */
export function getRuntimeInfo(): Promise<RuntimeInfo> {
  runtimeInfoPromise ??= (async () => {
    const [entropy, environment, versions] = await Promise.all([
      readHighEntropy(),
      getDesktopEnvironment(),
      readTauriVersions(),
    ]);

    const isMobile = environment.isMobile || isMobileDevice();
    const kind: RuntimeKind = !isTauri() ? "web" : isMobile ? "mobile" : "desktop";
    // Rust 侧 os 更可靠；浏览器环境下 desktop_environment 已做过 UA 兜底
    const os = formatOs(environment.os, entropy);

    return {
      appVersion: versions?.app ?? appInfo.version,
      webVersion: appInfo.version,
      tauriVersion: versions?.tauri ?? null,
      kind,
      os,
      arch: formatArch(entropy),
      engine: formatEngine(entropy),
    };
  })();

  return runtimeInfoPromise;
}
