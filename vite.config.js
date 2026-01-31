import { fileURLToPath, URL } from "node:url";
import { defineConfig, loadEnv } from "vite";
import { NaiveUiResolver } from "unplugin-vue-components/resolvers";
import { VitePWA } from "vite-plugin-pwa";
import vue from "@vitejs/plugin-vue";
import viteCompression from "vite-plugin-compression";
import AutoImport from "unplugin-auto-import/vite";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";
import { VueMcp } from 'vite-plugin-vue-mcp'
import vueDevTools from 'vite-plugin-vue-devtools'
import Components from "unplugin-vue-components/vite";
import MotionResolver from 'motion-v/resolver'

// https://vitejs.dev/config/
export default ({ mode }) =>
  defineConfig({
    plugins: [
      vue(),
      VueMcp(),
      vueDevTools(),
      wasm(),
      topLevelAwait({
        promiseExportName: '__tla',
        promiseImportName: (i) => `__tla_${i}`,
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
        resolvers: [
          NaiveUiResolver(),
          MotionResolver()
        ],
      }),
      // PWA
      VitePWA({
        registerType: "autoUpdate",
        workbox: {
          clientsClaim: true,
          skipWaiting: true,
          cleanupOutdatedCaches: true,
          runtimeCaching: [
            {
              urlPattern: /(.*?)\.(woff2|woff|ttf)/,
              handler: "CacheFirst",
              options: {
                cacheName: "file-cache",
              },
            },
            {
              urlPattern:
                /(.*?)\.(webp|png|jpe?g|svg|gif|bmp|psd|tiff|tga|eps)/,
              handler: "CacheFirst",
              options: {
                cacheName: "image-cache",
              },
            },
          ],
        },
        manifest: {
          name: loadEnv(mode, process.cwd()).VITE_SITE_TITLE,
          short_name: loadEnv(mode, process.cwd()).VITE_SITE_TITLE,
          description: loadEnv(mode, process.cwd()).VITE_SITE_DES,
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
      }),
      // viteCompression
      viteCompression(),
    ],
    server: {
      strictPort: true,
      port: 25536,
      open: true,
      http: true,
      ssr: true,
      proxy: {
        "/api/ncm": {
          target: loadEnv(mode, process.cwd()).VITE_MUSIC_API,
          changeOrigin: true,
          rewrite: (path) => path.replace(/^\/api\/ncm/, ""),
        },
        "/api/unm": {
          target: loadEnv(mode, process.cwd()).VITE_UNM_API,
          changeOrigin: true,
          rewrite: (path) => path.replace(/^\/api\/unm/, ""),
        },
        "/api/la": {
          target: loadEnv(mode, process.cwd()).VITE_LYRIC_ATLAS_API_URL,
          changeOrigin: true,
          rewrite: (path) => path.replace(/^\/api\/la/, ""),
        },
      },
    },
    envPrefix: ['VITE_', 'TAURI_PLATFORM', 'TAURI_ARCH', 'TAURI_FAMILY', 'TAURI_PLATFORM_VERSION', 'TAURI_PLATFORM_TYPE', 'TAURI_DEBUG'],
    resolve: {
      alias: {
        "@": fileURLToPath(new URL("./src", import.meta.url)),
        "vue-i18n": "vue-i18n/dist/vue-i18n.cjs.js",
      },
    },
    build: {
      // Tauri uses Chromium on Windows and WebKit on macOS and Linux
      target: 'esnext',
      // don't minify for debug builds
      minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
      compress: {
        pure_funcs: ["console.log"],
      },
      // 为调试构建生成源代码映射 (sourcemap)
      sourcemap: !!process.env.TAURI_DEBUG,
    },
    preview: {
      proxy: {
        "/api/ncm": {
          target: loadEnv(mode, process.cwd()).VITE_MUSIC_API,
          changeOrigin: true,
          rewrite: (path) => path.replace(/^\/api\/ncm/, ""),
        },
        "/api/unm": {
          target: loadEnv(mode, process.cwd()).VITE_UNM_API,
          changeOrigin: true,
          rewrite: (path) => path.replace(/^\/api\/unm/, ""),
        },
        "/api/la": {
          target: "https://lyric-atlas-api.vercel.app",
          changeOrigin: true,
          rewrite: (path) => path.replace(/^\/api\/la/, ""),
        },
      },
    },
  });
