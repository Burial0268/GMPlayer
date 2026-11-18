import { build } from "esbuild";
import { mkdir } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const directory = resolve(".cache/automix-curves-tests");
await mkdir(directory, { recursive: true });
const outfile = resolve(directory, "automix-curves.test.cjs");
await build({
  entryPoints: ["tests/automix-curves.test.ts"],
  outfile,
  bundle: true,
  platform: "node",
  format: "cjs",
  packages: "external",
  logLevel: "warning",
});
const result = spawnSync(process.execPath, ["--test", outfile], { stdio: "inherit" });
process.exitCode = result.status ?? 1;
