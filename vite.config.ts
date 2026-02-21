import { fileURLToPath, URL } from "node:url";
import { defineConfig, loadEnv } from "vite";
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

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd());
  const isTauriDebug = !!process.env.TAURI_DEBUG;
  const isTauri = !!process.env.TAURI_ENV_PLATFORM;

  return {
    plugins: [
      vue(),
      VueMcp(),
      vueDevTools(),
      wasm(),
      PkgConfig(),
      OptimizationPersist(),
      topLevelAwait({
        promiseExportName: "__tla",
        promiseImportName: (i: number) => `__tla_${i}`,
      }),
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
    server: {
      strictPort: true,
      port: 25536,
      open: true,
      http: true,
      ssr: true,
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
      sourcemap: isTauriDebug,
      rolldownOptions: {
        output: {
          // Oxc minifier options: drop console.log in production
          ...(!isTauriDebug && {
            minify: {
              compress: {
                dropConsole: true,
              },
            },
          }),
          advancedChunks: {
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
