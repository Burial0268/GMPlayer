import { build } from "esbuild";
import { mkdir } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const directory = resolve(".cache/last-backend-session-tests");
await mkdir(directory, { recursive: true });
const outfile = resolve(directory, "last-backend-session.test.cjs");
await build({
  entryPoints: ["tests/last-backend-session.test.ts"],
  outfile,
  bundle: true,
  platform: "node",
  format: "cjs",
  packages: "external",
  alias: { "@": resolve("src") },
  logLevel: "warning",
});
const result = spawnSync(process.execPath, ["--test", outfile], { stdio: "inherit" });
process.exitCode = result.status ?? 1;
