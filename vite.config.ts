import { fileURLToPath, URL } from "node:url";
import { Readable } from "node:stream";
import { createRequire } from "node:module";
import { defineConfig, loadEnv, ViteDevServer } from "vite";
import { NaiveUiResolver } from "unplugin-vue-components/resolvers";
import { VitePWA } from "vite-plugin-pwa";
import vue from "@vitejs/plugin-vue";
import { compression } from "vite-plugin-compression2";
import AutoImport from "unplugin-auto-import/vite";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";
import { VueMcp } from "vite-plugin-vue-mcp";
import vueDevTools from "vite-plugin-vue-devtools";
import Components from "unplugin-vue-components/vite";
import MotionResolver from "motion-v/resolver";
import OptimizationPersist from "vite-plugin-optimize-persist";
import PkgConfig from "vite-plugin-package-config";

const AUDIO_PROXY_PATH = "/api/audio-proxy";

// 只取应用真正用到的四个字段。直接 import package.json 会把整份依赖版本表
// 打进 main chunk（已验证产物中含完整 devDependencies 列表）。
const pkg = createRequire(import.meta.url)("./package.json");
const APP_INFO = {
  version: pkg.version,
  author: pkg.author,
  home: pkg.home,
  github: pkg.github,
};

function wasmBuildPlugins() {
  return [
    wasm(),
    topLevelAwait({
      promiseExportName: "__tla",
      promiseImportName: (i: number) => `__tla_${i}`,
    }),
  ];
}

function isBlockedAudioProxyHost(hostname: string): boolean {
  const host = hostname.toLowerCase();
  if (
    host === "localhost" ||
    host.endsWith(".localhost") ||
    host.endsWith(".local") ||
    host === "0.0.0.0" ||
    host === "127.0.0.1" ||
    host === "::1"
  ) {
    return true;
  }
  if (/^127\./.test(host) || /^10\./.test(host) || /^192\.168\./.test(host)) {
    return true;
  }
  const private172 = /^172\.(1[6-9]|2\d|3[0-1])\./;
  return private172.test(host);
}

function audioProxyPlugin() {
  return {
    name: "gmplayer-audio-proxy",
    configureServer(server: ViteDevServer) {
      server.middlewares.use(AUDIO_PROXY_PATH, async (req, res) => {
        const requestUrl = new URL(req.url || "", "http://localhost");
        const rawTarget = requestUrl.searchParams.get("url");
        if (!rawTarget) {
          res.statusCode = 400;
          res.end("missing url");
          return;
        }

        let target: URL;
        try {
          target = new URL(rawTarget);
        } catch {
          res.statusCode = 400;
          res.end("invalid url");
          return;
        }

        if (
          (target.protocol !== "http:" && target.protocol !== "https:") ||
          isBlockedAudioProxyHost(target.hostname)
        ) {
          res.statusCode = 400;
          res.end("unsupported url");
          return;
        }

        try {
          const headers = new Headers({
            Accept: "audio/*,*/*;q=0.8",
            Referer: `${target.origin}/`,
            "User-Agent":
              req.headers["user-agent"] || "Mozilla/5.0 AppleWebKit/537.36 Chrome Safari",
          });
          const range = req.headers.range;
          if (range) headers.set("Range", range);

          const upstream = await fetch(target, {
            headers,
            method: req.method === "HEAD" ? "HEAD" : "GET",
            redirect: "follow",
          });

          res.statusCode = upstream.status;
          for (const header of [
            "accept-ranges",
            "cache-control",
            "content-length",
            "content-range",
            "content-type",
            "etag",
            "last-modified",
          ]) {
            const value = upstream.headers.get(header);
            if (value) res.setHeader(header, value);
          }
          res.setHeader("x-audio-source-url", upstream.url || target.href);

          if (req.method === "HEAD" || !upstream.body) {
            res.end();
            return;
          }

          Readable.fromWeb(upstream.body as any).pipe(res);
        } catch (err) {
          res.statusCode = 502;
          res.end(err instanceof Error ? err.message : String(err));
        }
      });
    },
  };
}

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd());
  const isTauriDebug = !!process.env.TAURI_DEBUG;
  const isTauri = !!process.env.TAURI_ENV_PLATFORM;

  return {
    plugins: [
      vue(),
      VueMcp(),
      audioProxyPlugin(),
      vueDevTools(),
      ...wasmBuildPlugins(),
      PkgConfig(),
      OptimizationPersist(),
      AutoImport({
        imports: [
          "vue",
          {
            "naive-ui": ["useDialog", "useMessage", "useNotification", "useLoadingBar"],
          },
        ],
      }),
      Components({
        dts: true,
        resolvers: [NaiveUiResolver(), MotionResolver()],
      }),
      !isTauri
        ? VitePWA({
            registerType: "autoUpdate",
            workbox: {
              clientsClaim: true,
              skipWaiting: true,
              cleanupOutdatedCaches: true,
              runtimeCaching: [
                {
                  urlPattern: /(.*?)\.(woff2|woff|ttf)/,
                  handler: "CacheFirst",
                  options: { cacheName: "file-cache" },
                },
                {
                  urlPattern: /(.*?)\.(webp|png|jpe?g|svg|gif|bmp|psd|tiff|tga|eps)/,
                  handler: "CacheFirst",
                  options: { cacheName: "image-cache" },
                },
              ],
            },
            manifest: {
              name: env.VITE_SITE_TITLE,
              short_name: env.VITE_SITE_TITLE,
              description: env.VITE_SITE_DES,
              display: "standalone",
              start_url: "/",
              theme_color: "#fff",
              background_color: "#efefef",
              icons: [
                {
                  src: "/images/logo/favicon.png",
                  sizes: "200x200",
                  type: "image/png",
                },
              ],
            },
          })
        : null,
      compression({
        algorithms: ["gzip", "brotliCompress"],
      }),
    ].filter(Boolean),
    worker: {
      format: "es",
      plugins: wasmBuildPlugins,
    },
    server: {
      strictPort: true,
      port: 25536,
      open: true,
      watch: {
        ignored: ["**/src-tauri/**"],
      },
      headers: {
        "Cross-Origin-Opener-Policy": "same-origin",
        "Cross-Origin-Embedder-Policy": "credentialless",
      },
      proxy: {
        "/api/ncm": {
          target: env.VITE_MUSIC_API,
          changeOrigin: true,
          rewrite: (path: string) => path.replace(/^\/api\/ncm/, ""),
        },
        "/api/unm": {
          target: env.VITE_UNM_API,
          changeOrigin: true,
          rewrite: (path: string) => path.replace(/^\/api\/unm/, ""),
        },
      },
    },
    envPrefix: [
      "VITE_",
      "TAURI_PLATFORM",
      "TAURI_ARCH",
      "TAURI_FAMILY",
      "TAURI_PLATFORM_VERSION",
      "TAURI_PLATFORM_TYPE",
      "TAURI_DEBUG",
    ],
    define: {
      __GMPLAYER_TAURI_BUILD__: JSON.stringify(isTauri),
      __APP_INFO__: JSON.stringify(APP_INFO),
    },
    resolve: {
      alias: {
        ...(isTauri
          ? {
              "@/utils/AudioContext/AutoMix/TrackAnalyzerWorkerClient": fileURLToPath(
                new URL(
                  "./src/utils/AudioContext/AutoMix/TrackAnalyzerWorkerClient.disabled.ts",
                  import.meta.url,
                ),
              ),
            }
          : {}),
        "@": fileURLToPath(new URL("./src", import.meta.url)),
        "vue-i18n": "vue-i18n/dist/vue-i18n.cjs.js",
        ...(isTauri
          ? {
              "@player-helper/gmplayer-audio-backend": fileURLToPath(
                new URL("./src/utils/tauri/audio/stubs/disabledAudioBackendWasm.ts", import.meta.url),
              ),
              "@player-helper/audio-analysis": fileURLToPath(
                new URL("./src/utils/tauri/audio/stubs/disabledAudioAnalysisWasm.ts", import.meta.url),
              ),
            }
          : {}),
      },
    },
    build: {
      target: "esnext",
      minify: isTauriDebug ? false : "oxc",
      cssMinify: !isTauriDebug,
      sourcemap: isTauriDebug,
      rolldownOptions: {
        input: {
          main: fileURLToPath(new URL("./index.html", import.meta.url)),
          slave: fileURLToPath(new URL("./slave.html", import.meta.url)),
        },
        output: {
          // Oxc minifier options: drop console.log in production
          ...(!isTauriDebug && {
            minify: {
              compress: {
                dropConsole: true,
              },
            },
          }),
          codeSplitting: {
            groups: [
              // Vue core framework
              {
                name: "vue-core",
                test: /node_modules[\\/](vue|vue-router|pinia|pinia-plugin-persistedstate|vue-i18n|@vue)[\\/]/,
                priority: 20,
              },
              // Naive UI: heavy data components
              {
                name: "naive-ui-data",
                test: /node_modules[\\/]naive-ui[\\/]es[\\/](data-table|date-picker|time-picker|cascader|transfer|tree-select|tree|upload|calendar|log|mention|dynamic-input|auto-complete|color-picker)[\\/]/,
                priority: 17,
              },
              // Naive UI: form & input components
              {
                name: "naive-ui-form",
                test: /node_modules[\\/]naive-ui[\\/]es[\\/](form|input|input-number|select|checkbox|radio|switch|slider|rate|dynamic-tags|popselect|dropdown|menu|tabs|steps|pagination|collapse)[\\/]/,
                priority: 16,
              },
              // Naive UI: rest
              {
                name: "naive-ui",
                test: /node_modules[\\/]naive-ui[\\/]/,
                priority: 15,
              },
              // Icons
              {
                name: "icons",
                test: /node_modules[\\/]@vicons[\\/]/,
                priority: 15,
              },
              // PixiJS rendering
              {
                name: "pixi",
                test: /node_modules[\\/]@pixi[\\/]/,
                priority: 15,
              },
              // Media / player libs
              {
                name: "media",
                test: /node_modules[\\/](artplayer|plyr|swiper|screenfull)[\\/]/,
                priority: 10,
              },
              // Animation libs.
              //
              // gsap 和 motion 拆开：两者在同一块里时，分配采样只会给出
              // 混淆的 chunk 名（minify 后是 `h` / `g` 这种），没法判断
              // 该优化哪一个。motion-dom / motion-utils / framer-motion 是
              // motion-v 的运行时依赖，必须一起归组，否则会散进 vendor。
              {
                name: "gsap",
                test: /node_modules[\\/]gsap[\\/]/,
                priority: 10,
              },
              {
                name: "motion",
                test: /node_modules[\\/](motion-v|motion-dom|motion-utils|framer-motion)[\\/]/,
                priority: 10,
              },
              // AudioContext module
              {
                name: "audio-context",
                test: /src[\\/]utils[\\/]AudioContext[\\/]/,
                priority: 10,
              },
              // LyricsProcessor module
              {
                name: "lyrics-processor",
                test: /src[\\/]utils[\\/]LyricsProcessor[\\/]/,
                priority: 10,
              },
              // Tauri store 持久化层。只有 Tauri 里才会动态 import，必须自成
              // 一块——落进 vendor 的话会跟着首屏 chunk 一起预加载，Web 端白下。
              // @tauri-apps/api 要用更高优先级先抢走：它同时被首屏代码静态
              // import，跟着 @tauri-store 走的话会把整块拽成 eager chunk。
              {
                name: "tauri-api",
                test: /node_modules[\\/]@tauri-apps[\\/]/,
                priority: 16,
              },
              {
                name: "tauri-store",
                test: /node_modules[\\/]@tauri-store[\\/]/,
                priority: 15,
              },
              // Remaining vendor
              {
                name: "vendor",
                test: /node_modules[\\/]/,
                priority: 0,
                minSize: 20_000,
              },
            ],
          },
        },
      },
    },
    preview: {
      headers: {
        "Cross-Origin-Opener-Policy": "same-origin",
        "Cross-Origin-Embedder-Policy": "credentialless",
      },
      proxy: {
        "/api/ncm": {
          target: env.VITE_MUSIC_API,
          changeOrigin: true,
          rewrite: (path: string) => path.replace(/^\/api\/ncm/, ""),
        },
        "/api/unm": {
          target: env.VITE_UNM_API,
          changeOrigin: true,
          rewrite: (path: string) => path.replace(/^\/api\/unm/, ""),
        },
      },
    },
  };
});
