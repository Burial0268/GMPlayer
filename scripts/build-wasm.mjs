import { spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const cratePath = process.argv[2];

if (!cratePath) {
  console.error("Usage: node scripts/build-wasm.mjs <crate-path>");
  process.exit(2);
}

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const env = { ...process.env };

// Local sccache socket configuration can break cargo metadata/build on Windows.
// WASM package generation does not need the wrapper, so keep this path stable.
env.RUSTC_WRAPPER = "";

const result = spawnSync(
  "wasm-pack",
  ["build", "--release", "--scope", "player-helper", cratePath],
  {
    cwd: root,
    env,
    stdio: "inherit",
    shell: process.platform === "win32",
  },
);

if (result.error) {
  console.error(result.error.message);
}

const status = result.status ?? 1;
if (status === 0) {
  writeFileSync(
    resolve(root, cratePath, "pkg", ".gitignore"),
    [
      "*",
      "!.gitignore",
      "!package.json",
      "!*.js",
      "!*.ts",
      "!*.wasm",
      "!snippets/",
      "!snippets/**",
      "",
    ].join("\n"),
  );
}

process.exit(status);
