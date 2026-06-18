import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { exit } from "node:process";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const tauriConfPath = resolve(root, "src-tauri/tauri.conf.json");

console.log("Reading tauri.conf.json from", tauriConfPath);

const tauriConf = JSON.parse(readFileSync(tauriConfPath, "utf-8"));
const baseVersion = tauriConf.version;

if (!/^[0-9]+\.[0-9]+\.[0-9]+$/.test(baseVersion)) {
  console.error(`Invalid base version: ${baseVersion}`);
  exit(1);
}

const commitCount = execSync("git rev-list --count HEAD", {
  cwd: root,
})
  .toString()
  .trim();

const isIos = process.env.AMLL_IOS_BUILD === "true";

if (isIos) {
  tauriConf.version = baseVersion;
  tauriConf.bundle ??= {};
  tauriConf.bundle.iOS ??= {};
  tauriConf.bundle.iOS.bundleVersion = commitCount;
  console.log(`Generated iOS dev version: ${baseVersion} (${commitCount})`);
} else {
  const devVersion = `${baseVersion}+${commitCount}`;
  tauriConf.version = devVersion;
  console.log(`Generated dev version: ${baseVersion} -> ${devVersion}`);
}

writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, "\t"));
