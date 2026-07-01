import { fileURLToPath, URL } from "node:url";
import { Readable } from "node:stream";
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
import OptimizationPersist from 'vite-plugin-optimize-persist'
import PkgConfig from 'vite-plugin-package-config'

const AUDIO_PROXY_PATH = "/api/audio-proxy";

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
              req.headers["user-agent"] ||
              "Mozilla/5.0 AppleWebKit/537.36 Chrome Safari",
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
            "naive-ui": [
              "useDialog",
              "useMessage",
              "useNotification",
              "useLoadingBar",
            ],
          },
        ],
      }),
      Components({
        dts: true,
        resolvers: [NaiveUiResolver(), MotionResolver()],
      }),
      !isTauri ? VitePWA({
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
              urlPattern:
                /(.*?)\.(webp|png|jpe?g|svg|gif|bmp|psd|tiff|tga|eps)/,
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
      }) : null,
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
      http: true,
      ssr: true,
      watch: {
        ignored: ['**/src-tauri/**'],
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
    resolve: {
      alias: {
        "@": fileURLToPath(new URL("./src", import.meta.url)),
        "vue-i18n": "vue-i18n/dist/vue-i18n.cjs.js",
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
              // Animation libs
              {
                name: "animation",
                test: /node_modules[\\/](gsap|motion-v)[\\/]/,
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
