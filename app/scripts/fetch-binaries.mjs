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
// Windows 10+ (bsdtar). Windows-only target (ADR-011); a no-op on other OSes.

import { existsSync } from "node:fs";
import {
  copyFile,
  mkdir,
  readdir,
  rename,
  rm,
  stat,
} from "node:fs/promises";
import { createWriteStream } from "node:fs";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";

// Resolve paths relative to THIS script so CWD doesn't matter.
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url)); // app/scripts
const BINARIES_DIR = resolve(SCRIPT_DIR, "..", "src-tauri", "binaries");

// The native engines MIA spawns as warm sidecars. whisper-server (STT) sits at the
// top of binaries/; llama-server (optional Phase-2 LLM) goes in a binaries/llama/
// SUBDIR — both ship same-named ggml*.dll, so isolating each server with its own DLLs
// avoids a clash (Windows resolves DLLs from the exe's own directory).
const ENGINES = [
  {
    // CPU whisper.cpp Windows x64 release (matches the v1.8.4 GPU build in stt.rs).
    url: "https://github.com/ggml-org/whisper.cpp/releases/download/v1.8.4/whisper-bin-x64.zip",
    exe: "whisper-server.exe",
    destDir: BINARIES_DIR,
    zipName: "whisper-cpu.zip",
  },
  {
    // CPU llama.cpp Windows x64 release (has llama-server.exe; ai_commands.rs).
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b9410/llama-b9410-bin-win-cpu-x64.zip",
    exe: "llama-server.exe",
    destDir: join(BINARIES_DIR, "llama"),
    zipName: "llama-cpu.zip",
  },
];

function fail(message) {
  console.error(`fetch-binaries: ${message}`);
  process.exit(1);
}

/** Download a URL (following redirects) to `dest`, via a `.part` temp + atomic rename. */
async function downloadFile(url, dest) {
  const res = await fetch(url, { redirect: "follow" });
  if (!res.ok || !res.body) {
    throw new Error(`download failed (${res.status} ${res.statusText}) for ${url}`);
  }
  const part = `${dest}.part`;
  await pipeline(Readable.fromWeb(res.body), createWriteStream(part));
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

/** Copy `exeName` + every sibling *.dll from `srcDir` into `destDir`. */
async function copyEngineFiles(srcDir, destDir, exeName) {
  const copied = [];
  for (const entry of await readdir(srcDir, { withFileTypes: true })) {
    if (!entry.isFile()) continue;
    const lower = entry.name.toLowerCase();
    if (entry.name === exeName || lower.endsWith(".dll")) {
      await copyFile(join(srcDir, entry.name), join(destDir, entry.name));
      copied.push(entry.name);
    }
  }
  return copied;
}

/** Fetch one engine: download release zip → extract → copy exe + sibling DLLs. */
async function fetchEngine({ url, exe, destDir, zipName }) {
  await mkdir(destDir, { recursive: true });
  // Idempotent: skip if the engine binary is already present.
  if (existsSync(join(destDir, exe))) {
    console.log(`fetch-binaries: ${exe} already present in ${destDir}`);
    return;
  }
  const zip = join(destDir, zipName);
  const tmp = join(destDir, "extract");
  try {
    console.log(`fetch-binaries: downloading ${url}`);
    await downloadFile(url, zip);
    console.log("fetch-binaries: extracting...");
    await extractZip(zip, tmp);
    const found = await findFile(tmp, exe);
    if (!found) throw new Error(`${exe} not found in the downloaded archive`);
    const copied = await copyEngineFiles(dirname(found), destDir, exe);
    if (!copied.includes(exe)) throw new Error(`failed to copy ${exe}`);
    console.log(`fetch-binaries: copied into ${destDir}:\n  ${copied.join("\n  ")}`);
  } finally {
    await rm(zip, { force: true }).catch(() => {});
    await rm(tmp, { recursive: true, force: true }).catch(() => {});
  }
  const out = join(destDir, exe);
  if (!existsSync(out)) fail(`${exe} missing after fetch`);
  const { size } = await stat(out);
  console.log(`fetch-binaries: done (${exe} = ${size} bytes)`);
}

async function main() {
  // Binaries are Windows-only (ADR-011) — don't fail dev on macOS/Linux.
  if (process.platform !== "win32") {
    console.log(
      "fetch-binaries: engines are Windows-only (ADR-011); nothing to fetch on " +
        `${process.platform}.`,
    );
    return;
  }
  for (const engine of ENGINES) {
    await fetchEngine(engine);
  }
}

main().catch((err) => fail(err?.message ?? String(err)));
