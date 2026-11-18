// 外链统一出口。Tauri 下交给 opener 插件用系统默认浏览器打开，避免 release
// 构建里 WebView 直接导航到外站后回不来；Web 下保持新标签页打开的既有行为。
import { isTauri } from "@/utils/tauri/core/runtime";

/** 允许交给系统默认应用处理的协议，与 opener 能力里的 scope 保持一致 */
const EXTERNAL_PROTOCOLS = new Set(["http:", "https:", "mailto:", "tel:"]);

const resolveUrl = (url: string): URL | null => {
  try {
    return new URL(url, window.location.href);
  } catch {
    return null;
  }
};

/** 是否为需要跳出应用的外部链接（同源路径视为站内导航） */
export function isExternalUrl(url: string): boolean {
  const parsed = resolveUrl(url);
  if (!parsed) return false;
  if (!EXTERNAL_PROTOCOLS.has(parsed.protocol)) return false;
  if (parsed.protocol === "mailto:" || parsed.protocol === "tel:") return true;
  return parsed.origin !== window.location.origin;
}

/** 打开外链：Tauri 走系统浏览器，Web 走新标签页 */
export async function openExternalUrl(url: string): Promise<void> {
  if (!url) return;

  if (!isTauri()) {
    // 不传 windowFeatures，否则部分浏览器会开成弹出窗口而非新标签页
    const opened = window.open(url, "_blank");
    if (opened) opened.opener = null;
    return;
  }

  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } catch (error) {
    console.error("Failed to open external url:", url, error);
  }
}

/**
 * 打开链接：外链交给 openExternalUrl，站内链接维持 window.open 的原有语义
 * （Web 下新标签页；Tauri 下由调用方保证只在需要时使用）。
 */
export function openLink(url: string): void {
  if (!url) return;
  if (isExternalUrl(url)) {
    void openExternalUrl(url);
    return;
  }
  window.open(url);
}

const shouldSkipAnchor = (event: MouseEvent): boolean => {
  if (event.defaultPrevented || event.button !== 0) return true;
  // Web 下让浏览器自己处理修饰键点击（新窗口 / 下载等）；Tauri 里没有这些语义
  return !isTauri() && (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey);
};

let anchorInterceptorInstalled = false;

/**
 * 全局兜底：拦截所有 <a> 上的外链点击（含 markdown / v-html 里的运行时链接），
 * 统一走 openExternalUrl。需在应用启动时调用一次。
 */
export function installExternalLinkInterceptor(): void {
  if (anchorInterceptorInstalled || typeof document === "undefined") return;
  anchorInterceptorInstalled = true;

  document.addEventListener(
    "click",
    (event) => {
      if (shouldSkipAnchor(event)) return;

      const anchor = (event.target as Element | null)?.closest?.<HTMLAnchorElement>("a[href]");
      const href = anchor?.getAttribute("href");
      if (!href || href.startsWith("#")) return;
      if (!isExternalUrl(href)) return;

      event.preventDefault();
      void openExternalUrl(anchor!.href);
    },
    true,
  );
}
