// `pnpm tauri` shim. Everything is forwarded to @tauri-apps/cli untouched
// except `tauri android …`, which first gets the cross-compilation environment
// the CLI does not set for us.
//
// The part that blocks a build outright is bindgen's. rquickjs-sys ships
// pre-generated bindings for 16 desktop targets and none for *-linux-android,
// so ncm-core's QuickJS has to be bound on the fly — and bindgen finds neither
// the NDK sysroot (where Bionic's headers are) nor the clang resource dir
// (where the compiler-private stdbool.h / stddef.h / stdatomic.h are). It then
// fails with `'stdbool.h' file not found`, an error that names nothing about
// QuickJS, libclang or the NDK.
//
// The sysroot can only come from the NDK, but the resource dir has to come from
// whichever LLVM actually provided the libclang bindgen loaded: pairing NDK
// clang's copy with a standalone libclang is a version mismatch bought for
// nothing. So both are picked together, and the version is asked of `clang`
// rather than written down (NDK 27 is clang 18, NDK 29 is 21).
//
// This mirrors the Android job in .github/workflows/build-app.yml — keep the
// two in step. Anything already exported wins, so a BINDGEN_EXTRA_CLANG_ARGS_*
// set by hand is never overwritten.

import { execFileSync, spawn } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const HOST_TAGS = {
  win32: "windows-x86_64",
  darwin: "darwin-x86_64",
  linux: "linux-x86_64",
};

// Rust target -> clang wrapper triple. Not one-to-one: all three arm variants
// compile to armv7a-linux-androideabi.
const ANDROID_TARGETS = [
  ["aarch64_linux_android", "aarch64-linux-android"],
  ["armv7_linux_androideabi", "armv7a-linux-androideabi"],
  ["thumbv7neon_linux_androideabi", "armv7a-linux-androideabi"],
  ["arm_linux_androideabi", "armv7a-linux-androideabi"],
  ["i686_linux_android", "i686-linux-android"],
  ["x86_64_linux_android", "x86_64-linux-android"],
];

const EXE = process.platform === "win32" ? ".exe" : "";
const LIBCLANG = /^libclang\.(dll|dylib|so)($|\.)/;

const warn = (message) => console.warn(`[tauri android] ${message}`);

// clang takes forward slashes on Windows too, and bindgen splits
// BINDGEN_EXTRA_CLANG_ARGS shlex-style, where a backslash escapes rather than
// separates — so a native Windows path would arrive with its separators eaten.
const clangArg = (path) => {
  const slashed = path.replaceAll("\\", "/");
  return /\s/.test(slashed) ? `"${slashed}"` : slashed;
};

const compareVersions = (a, b) => {
  const parts = (name) => name.split(".").map((part) => Number.parseInt(part, 10) || 0);
  const [left, right] = [parts(a), parts(b)];
  for (let i = 0; i < Math.max(left.length, right.length); i += 1) {
    const diff = (right[i] ?? 0) - (left[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
};

// An explicit NDK_HOME wins; otherwise take the newest NDK the SDK has, which
// is what the Tauri CLI's own lookup does.
const findNdk = (env) => {
  for (const key of ["NDK_HOME", "ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"]) {
    if (env[key] && existsSync(env[key])) return env[key];
  }
  for (const key of ["ANDROID_HOME", "ANDROID_SDK_ROOT"]) {
    const ndkRoot = env[key] ? join(env[key], "ndk") : null;
    if (!ndkRoot || !existsSync(ndkRoot)) continue;
    const newest = readdirSync(ndkRoot, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => entry.name)
      .sort(compareVersions)[0];
    if (newest) return join(ndkRoot, newest);
  }
  return null;
};

// bindgen needs libclang — clang's *C API shared library*. The NDK is a
// cross-compilation toolchain, not an LLVM distribution meant to be used as a
// parser by third-party tools, so whether it ships one at all depends on the
// release and the host. A standalone LLVM is preferred and the NDK's own copy
// is only a fallback; either way the matching `clang` comes from the same root,
// because it is what we ask for the resource dir.
const findLibclang = (env, ndkLlvm) => {
  const candidates = [
    env.LIBCLANG_PATH,
    join(ndkLlvm, "bin"),
    join(ndkLlvm, "lib"),
    join(ndkLlvm, "lib64"),
  ];
  for (const dir of candidates) {
    if (!dir || !existsSync(dir)) continue;
    if (!readdirSync(dir).some((name) => LIBCLANG.test(name))) continue;
    for (const clang of [join(dir, `clang${EXE}`), join(dirname(dir), "bin", `clang${EXE}`)]) {
      if (existsSync(clang)) return { libclangDir: dir, clang };
    }
  }
  return null;
};

// minSdk is not a compatibility knob here: it selects which sysroot directory
// cargo-ndk links against, and libaaudio.so — cpal 0.18's Android backend —
// first appears at usr/lib/<triple>/26. Reading it rather than restating it is
// what keeps this from drifting away from the Gradle project.
const findApiLevel = () => {
  const conf = resolve(root, "src-tauri/tauri.android.conf.json");
  const gradle = resolve(root, "src-tauri/gen/android/app/build.gradle.kts");
  let level = null;
  try {
    level = JSON.parse(readFileSync(conf, "utf8"))?.bundle?.android?.minSdkVersion ?? null;
  } catch (error) {
    warn(`could not read ${conf}: ${error.message}`);
  }
  if (existsSync(gradle)) {
    const declared = /^\s*minSdk\s*=\s*(\d+)/m.exec(readFileSync(gradle, "utf8"))?.[1];
    if (declared && level !== null && Number(declared) !== Number(level)) {
      warn(`minSdkVersion ${level} disagrees with the Gradle project's minSdk ${declared}`);
    }
    if (level === null && declared) level = Number(declared);
  }
  return level;
};

const applyAndroidEnv = (env) => {
  const hostTag = HOST_TAGS[process.platform];
  if (!hostTag) return warn(`no known NDK prebuilt directory for ${process.platform}`);

  const ndk = findNdk(env);
  if (!ndk) {
    return warn(
      "no NDK found — set NDK_HOME, or install one under $ANDROID_HOME/ndk. " +
        "Without it QuickJS fails to build with `'stdbool.h' file not found`.",
    );
  }

  const ndkLlvm = join(ndk, "toolchains", "llvm", "prebuilt", hostTag);
  const sysroot = join(ndkLlvm, "sysroot");
  if (!existsSync(sysroot)) return warn(`the NDK at ${ndk} has no sysroot for ${hostTag}`);

  const found = findLibclang(env, ndkLlvm);
  if (!found) {
    return warn(
      "no libclang found — set LIBCLANG_PATH to the bin/lib directory of an LLVM " +
        "installation. bindgen cannot run without it.",
    );
  }

  let resourceInclude = null;
  try {
    const dir = execFileSync(found.clang, ["-print-resource-dir"], { encoding: "utf8" }).trim();
    resourceInclude = join(dir, "include");
  } catch (error) {
    return warn(`\`${found.clang} -print-resource-dir\` failed: ${error.message}`);
  }
  if (!existsSync(join(resourceInclude, "stdbool.h"))) {
    warn(`${resourceInclude} has no stdbool.h; bindgen will probably fail`);
  }

  const apiLevel = findApiLevel();
  if (apiLevel === null) return warn("could not determine the Android API level");

  const args = `--sysroot=${clangArg(sysroot)} -I${clangArg(resourceInclude)}`;
  for (const [rustTarget, clangTriple] of ANDROID_TARGETS) {
    const key = `BINDGEN_EXTRA_CLANG_ARGS_${rustTarget}`;
    if (env[key]) continue;
    env[key] = `--target=${clangTriple}${apiLevel} ${args}`;
  }

  // Assigned rather than defaulted: `findLibclang` already prefers whatever
  // LIBCLANG_PATH pointed at, so this is a no-op in the normal case and a
  // correction when that path held no libclang — in which case leaving it be
  // would have bindgen search PATH and load a different LLVM than the resource
  // dir above came from, which is the mismatch this whole shim exists to avoid.
  env.LIBCLANG_PATH = found.libclangDir;
  env.NDK_HOME ??= ndk;
  env.ANDROID_NDK_HOME ??= ndk;
  env.ANDROID_NDK_ROOT ??= ndk;
  env.CARGO_NDK_ANDROID_PLATFORM ??= String(apiLevel);

  console.log(
    `[tauri android] api=${apiLevel} ndk=${ndk}\n` +
      `[tauri android] libclang=${found.libclangDir}\n` +
      `[tauri android] resource dir=${resourceInclude}`,
  );
};

const argv = process.argv.slice(2);
const env = { ...process.env };

if (argv[0] === "android") applyAndroidEnv(env);

const cli = createRequire(import.meta.url).resolve("@tauri-apps/cli/tauri.js");
const child = spawn(process.execPath, [cli, ...argv], { env, stdio: "inherit" });

child.on("error", (error) => {
  console.error(error.message);
  process.exit(1);
});
child.on("exit", (code, signal) => {
  // Forward the signal rather than an exit code so Ctrl-C on `android dev`
  // still looks like an interrupt to whatever ran pnpm.
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
