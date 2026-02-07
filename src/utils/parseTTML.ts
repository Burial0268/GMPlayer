/**
 * TTML Parser Wrapper
 * 封装 @lyrics-helper-rs/ttml-processor WASM 模块
 * 提供懒加载初始化和 TTML 解析功能
 */

// @ts-ignore
import { init, parse_ttml } from "@lyrics-helper-rs/ttml-processor";

let isLoaded = false;
let initPromise: Promise<void> | null = null;

/**
 * 确保 WASM 模块已加载
 * 首次调用时初始化，后续调用直接返回
 */
export async function ensureTTMLLoaded(): Promise<void> {
	if (isLoaded) return;
	if (initPromise) return initPromise;

	initPromise = (async () => {
		try {
			init();
			isLoaded = true;
			console.log("[parseTTML] WASM ttml-processor 初始化成功");
		} catch (error) {
			console.error("[parseTTML] WASM ttml-processor 初始化失败", error);
			isLoaded = false;
			initPromise = null;
			throw error;
		}
	})();

	return initPromise;
}

/**
 * 解析 TTML 格式歌词
 * 使用 @lyrics-helper-rs/ttml-processor WASM 模块
 * 需要先调用 ensureTTMLLoaded() 确保 WASM 已加载
 *
 * @param ttmlContent TTML XML 字符串
 * @returns 解析后的歌词数据，包含 lines 和 metadata
 */
export function parseTTML(ttmlContent: string): any {
	if (!isLoaded) {
		throw new Error("[parseTTML] WASM not loaded, call ensureTTMLLoaded() first");
	}

	return parse_ttml(ttmlContent);
}
