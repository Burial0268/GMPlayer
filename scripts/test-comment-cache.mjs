import { build } from "esbuild";
import { mkdir } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const directory = resolve(".cache/comment-cache-tests");
await mkdir(directory, { recursive: true });
const outfile = resolve(directory, "comment-cache.test.cjs");
await build({
  entryPoints: ["tests/comment-cache.test.ts"],
  outfile,
  bundle: true,
  platform: "node",
  format: "cjs",
  packages: "external",
  alias: { "@": resolve("src") },
  logLevel: "warning",
  plugins: [
    {
      name: "stub-ncm-request",
      setup(b) {
        b.onResolve({ filter: /^@\/utils\/ncm\/request$/ }, () => ({
          path: "ncm-request",
          namespace: "stub",
        }));
        b.onLoad({ filter: /.*/, namespace: "stub" }, () => ({
          contents: "export default (config) => globalThis.__commentRequest(config);",
          loader: "js",
        }));
      },
    },
  ],
});
const result = spawnSync(process.execPath, ["--test", outfile], { stdio: "inherit" });
process.exitCode = result.status ?? 1;
