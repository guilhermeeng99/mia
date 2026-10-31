// Fetch the native whisper engine binary for MIA.
//
// WHY: MIA's STT is a warm/resident `whisper-server` sidecar (ADR-004) — the engine
// binary is `whisper-server.exe`, not a per-utterance `whisper-cli` spawn. At runtime
// `stt::server_exe()` looks for it (in dev) at `app/src-tauri/binaries/whisper-server.exe`,
// and (in a bundle) at `resource_dir/binaries/whisper-server.exe`. This script populates
// `app/src-tauri/binaries/` so both `tauri dev` finds it and `tauri build` can ship it
// via `bundle.resources` ("binaries/*").
//
// It mirrors the GPU path in stt.rs (download release zip → `tar -xf` extract → copy
// whisper-server.exe + sibling *.dll), but for the CPU x64 build. Runs under both Node
// (18+) and Bun, ESM, no external deps — uses global `fetch` and the `tar` that ships on
// Windows 10+ (bsdtar).
//
// macOS: whisper.cpp publishes no prebuilt server for macOS, so we build it from the
// same pinned tag with CMake — static (no dylibs to bundle) and with the Metal shaders
// embedded, so Apple Silicon gets GPU inference out of the box. Needs `git` + `cmake`
// (Xcode Command Line Tools + `brew install cmake`). The output is named with the Rust
// target triple (`whisper-server-aarch64-apple-darwin`) because macOS ships it as a
// Tauri `externalBin` sidecar (tauri.macos.conf.json): Tauri strips the suffix, puts it
// in `Contents/MacOS` and code-signs it with the app — a Mach-O left in `Resources`
// is never signed, which notarization rejects. Other OSes are a no-op.

import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { createReadStream, createWriteStream } from "node:fs";
import { copyFile, mkdir, readdir, rename, rm, stat } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";

// CPU whisper.cpp Windows x64 release (matches the v1.8.4 GPU build in stt.rs).
const CPU_URL =
  "https://github.com/ggml-org/whisper.cpp/releases/download/v1.8.4/whisper-bin-x64.zip";
const CPU_SHA256 = "74f973345cb52ef5ba3ec9e7e7af8e48cc8c71722d1528603b80588a11f82e3e";

const SERVER_EXE = "whisper-server.exe";

// Same pinned release as the Windows zip, built from source on macOS.
const WHISPER_TAG = "v1.8.4";
const WHISPER_REPO = "https://github.com/ggml-org/whisper.cpp.git";
const MAC_SERVER_BIN = "whisper-server";
// Tauri's externalBin naming: `<name>-<rust target triple>`.
const MAC_TRIPLE = process.arch === "arm64" ? "aarch64-apple-darwin" : "x86_64-apple-darwin";
const MAC_SIDECAR = `${MAC_SERVER_BIN}-${MAC_TRIPLE}`;

// Resolve paths relative to THIS script so CWD doesn't matter.
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url)); // app/scripts
const BINARIES_DIR = resolve(SCRIPT_DIR, "..", "src-tauri", "binaries");

function fail(message) {
  console.error(`fetch-binaries: ${message}`);
  process.exit(1);
}

async function sha256File(path) {
  const hash = createHash("sha256");
  await pipeline(createReadStream(path), hash);
  return hash.digest("hex");
}

async function verifySha256(path, expected) {
  const got = await sha256File(path);
  if (got.toLowerCase() !== expected.toLowerCase()) {
    throw new Error(`integrity check failed for ${path}: expected ${expected}, got ${got}`);
  }
}

/** Download a URL (following redirects) to `dest`, via a verified `.part` temp + atomic rename. */
async function downloadFile(url, dest, expectedSha256) {
  const res = await fetch(url, { redirect: "follow" });
  if (!res.ok || !res.body) {
    throw new Error(`download failed (${res.status} ${res.statusText}) for ${url}`);
  }
  const part = `${dest}.part`;
  await pipeline(Readable.fromWeb(res.body), createWriteStream(part));
  await verifySha256(part, expectedSha256);
  await rename(part, dest);
}

/** Extract a zip into `tmp` using the bundled `tar` (bsdtar on Windows 10+). */
async function extractZip(zip, tmp) {
  await rm(tmp, { recursive: true, force: true });
  await mkdir(tmp, { recursive: true });
  await new Promise((res, rej) => {
    const child = spawn("tar", ["-xf", zip, "-C", tmp], { stdio: "inherit" });
    child.on("error", rej);
    child.on("close", (code) =>
      code === 0 ? res() : rej(new Error(`tar exited with code ${code}`)),
    );
  });
}

/** Recursively find a file by name; returns its absolute path or null. */
async function findFile(dir, name) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      const hit = await findFile(full, name);
      if (hit) return hit;
    } else if (entry.name === name) {
      return full;
    }
  }
  return null;
}

/** Copy whisper-server.exe + every sibling *.dll from `srcDir` into `destDir`. */
async function copyEngineFiles(srcDir, destDir) {
  const copied = [];
  for (const entry of await readdir(srcDir, { withFileTypes: true })) {
    if (!entry.isFile()) continue;
    const lower = entry.name.toLowerCase();
    if (entry.name === SERVER_EXE || lower.endsWith(".dll")) {
      await copyFile(join(srcDir, entry.name), join(destDir, entry.name));
      copied.push(entry.name);
    }
  }
  return copied;
}

/** Run a command, inheriting stdio; rejects on a non-zero exit. */
function run(cmd, args, cwd) {
  return new Promise((res, rej) => {
    const child = spawn(cmd, args, { stdio: "inherit", cwd });
    child.on("error", rej);
    child.on("close", (code) =>
      code === 0 ? res() : rej(new Error(`${cmd} exited with code ${code}`)),
    );
  });
}

/** macOS: build a static, Metal-enabled whisper-server from the pinned tag. */
async function buildMacServer() {
  await mkdir(BINARIES_DIR, { recursive: true });
  const out = join(BINARIES_DIR, MAC_SIDECAR);
  // Builds from before the sidecar switch left an unsuffixed binary; reuse it.
  const legacy = join(BINARIES_DIR, MAC_SERVER_BIN);
  if (!existsSync(out) && existsSync(legacy)) await rename(legacy, out);
  if (existsSync(out)) {
    console.log(`fetch-binaries: ${MAC_SIDECAR} already present in ${BINARIES_DIR}`);
    return;
  }
  const src = join(BINARIES_DIR, "whisper-src");
  try {
    await rm(src, { recursive: true, force: true });
    console.log(`fetch-binaries: cloning whisper.cpp ${WHISPER_TAG}`);
    await run("git", ["clone", "--depth", "1", "--branch", WHISPER_TAG, WHISPER_REPO, src]);
    // WHY these flags: static libs → a single self-contained binary for the .app bundle;
    // embedded Metal library → no default.metallib to ship; no tests/examples beyond
    // the server keeps the build short.
    await run(
      "cmake",
      [
        "-B",
        "build",
        "-DCMAKE_BUILD_TYPE=Release",
        "-DBUILD_SHARED_LIBS=OFF",
        "-DGGML_METAL=ON",
        "-DGGML_METAL_EMBED_LIBRARY=ON",
        "-DWHISPER_BUILD_TESTS=OFF",
        "-DWHISPER_BUILD_EXAMPLES=ON",
        "-DWHISPER_BUILD_SERVER=ON",
      ],
      src,
    );
    await run(
      "cmake",
      ["--build", "build", "--config", "Release", "-j", "--target", "whisper-server"],
      src,
    );
    const built = await findFile(join(src, "build"), MAC_SERVER_BIN);
    if (!built) throw new Error(`${MAC_SERVER_BIN} not produced by the build`);
    await copyFile(built, out);
  } finally {
    await rm(src, { recursive: true, force: true }).catch(() => {});
  }
  const { size } = await stat(out);
  console.log(`fetch-binaries: done (${MAC_SIDECAR} = ${size} bytes)`);
}

async function main() {
  if (process.platform === "darwin") {
    await buildMacServer();
    return;
  }
  // No prebuilt engine for other platforms — don't fail dev on Linux.
  if (process.platform !== "win32") {
    console.log(
      `fetch-binaries: whisper-server is Windows-only (ADR-011); nothing to fetch on ${process.platform}.`,
    );
    return;
  }

  await mkdir(BINARIES_DIR, { recursive: true });

  // Idempotent: skip if the engine binary is already present.
  if (existsSync(join(BINARIES_DIR, SERVER_EXE))) {
    console.log(`fetch-binaries: ${SERVER_EXE} already present in ${BINARIES_DIR}`);
    return;
  }

  const zip = join(BINARIES_DIR, "whisper-cpu.zip");
  const tmp = join(BINARIES_DIR, "extract");
  try {
    console.log(`fetch-binaries: downloading ${CPU_URL}`);
    await downloadFile(CPU_URL, zip, CPU_SHA256);

    console.log("fetch-binaries: extracting...");
    await extractZip(zip, tmp);

    const exe = await findFile(tmp, SERVER_EXE);
    if (!exe) throw new Error(`${SERVER_EXE} not found in the downloaded archive`);

    const copied = await copyEngineFiles(dirname(exe), BINARIES_DIR);
    if (!copied.includes(SERVER_EXE)) {
      throw new Error(`failed to copy ${SERVER_EXE}`);
    }
    console.log(`fetch-binaries: copied into ${BINARIES_DIR}:\n  ${copied.join("\n  ")}`);
  } finally {
    await rm(zip, { force: true }).catch(() => {});
    await rm(tmp, { recursive: true, force: true }).catch(() => {});
  }

  // Sanity check.
  const out = join(BINARIES_DIR, SERVER_EXE);
  if (!existsSync(out)) fail(`${SERVER_EXE} missing after fetch`);
  const { size } = await stat(out);
  console.log(`fetch-binaries: done (${SERVER_EXE} = ${size} bytes)`);
}

main().catch((err) => fail(err?.message ?? String(err)));
