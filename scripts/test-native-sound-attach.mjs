import { build } from "esbuild";
import { mkdir } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const directory = resolve(".cache/native-sound-attach-tests");
await mkdir(directory, { recursive: true });
const outfile = resolve(directory, "native-sound-attach.test.cjs");
await build({
  entryPoints: ["tests/native-sound-attach.test.ts"],
  outfile,
  bundle: true,
  platform: "node",
  format: "cjs",
  packages: "external",
  logLevel: "warning",
});
const result = spawnSync(process.execPath, ["--test", outfile], { stdio: "inherit" });
process.exitCode = result.status ?? 1;
