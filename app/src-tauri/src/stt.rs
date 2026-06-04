//! Speech-to-text engine — a **warm/resident `whisper-server`** sidecar (ADR-004
//! MVP default; cmake-free, reuses Toolzy's binary-fetch + download machinery).
//! The model loads once when the server starts; each utterance is a localhost
//! POST of in-memory PCM (never disk, ADR-001). `whisper-rs` in-process is the
//! later optimization behind the same `SttBackend` seam.
//!
//! The registry / download / GPU-engine code follows the proven file-transcription
//! pattern documented in `docs/specs/speech-to-text.md`, adapted to MIA's warm live path.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

/// Hugging Face source for Whisper ggml models (one resolve URL per file).
const HF_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
/// Silero VAD model — passed to `whisper-server` (`server_args` → `--vad-model`) for
/// server-side silence gating / anti-hallucination, NOT to `vad.rs` (which is a pure
/// probability-fed state machine that loads no model). Same `.bin` Toolzy fetches.
const VAD_FILENAME: &str = "ggml-silero-v6.2.0.bin";
const VAD_URL: &str =
    "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin";
const VAD_SHA256: &str = "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987";
const VAD_SIZE_BYTES: u64 = 885_098;
/// Self-contained NVIDIA (CUDA) whisper.cpp build — bundles cuBLAS DLLs, needs only
/// an NVIDIA driver. Downloaded on demand (~435 MB) for the GPU speedup.
const GPU_URL: &str =
    "https://github.com/ggml-org/whisper.cpp/releases/download/v1.8.4/whisper-cublas-12.4.0-bin-x64.zip";
const GPU_SHA256: &str = "b07cff4e59831b227896018facbb6334907bf324a342c84597c44f087823d252";

/// A model offered in the UI. For *dictation* the default favours latency, so the
/// list leads with `small` (see `docs/specs/speech-to-text.md` §4).
struct ModelDef {
    id: &'static str,
    label: &'static str,
    size_mb: u32,
    size_bytes: u64,
    // The latency-friendly default for live dictation; the UI flags it so both the
    // onboarding picker and the Hub stay consistent (docs/specs/onboarding.md Rule 7).
    recommended: bool,
}

const MODELS: &[ModelDef] = &[
    ModelDef { id: "small", label: "Small", size_mb: 466, size_bytes: 487_601_967, recommended: true },
    ModelDef { id: "medium", label: "Medium", size_mb: 1500, size_bytes: 1_533_763_059, recommended: false },
    ModelDef { id: "large-v3-turbo", label: "Large v3 Turbo", size_mb: 1600, size_bytes: 1_624_555_275, recommended: false },
    ModelDef { id: "large-v3", label: "Large v3", size_mb: 3100, size_bytes: 3_095_033_483, recommended: false },
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperModel {
    id: String,
    label: String,
    size_mb: u32,
    downloaded: bool,
    recommended: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuStatus {
    gpu_present: bool,
    downloaded: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarmStatus {
    loaded: bool,
    model: Option<String>,
    backend: String,
    gpu: bool,
}

/// One-time model download progress, streamed to the UI over a Tauri `Channel`.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    percent: f64,
    downloaded: u64,
    total: Option<u64>,
}

/// The resident whisper-server process + which model it has loaded.
struct WarmServer {
    child: Child,
    port: u16,
    model: String,
    gpu: bool,
}

impl Drop for WarmServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Managed Tauri state: the warm server (loaded once, shared across utterances).
#[derive(Default)]
pub struct SttState {
    server: Mutex<Option<WarmServer>>,
    downloads: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure helpers (cargo-tested)
// ─────────────────────────────────────────────────────────────────────────────

/// `ggml-<id>.bin` for a known model id, else `None`.
fn model_def(id: &str) -> Option<&'static ModelDef> {
    MODELS.iter().find(|m| m.id == id)
}

fn model_filename(id: &str) -> Option<String> {
    model_def(id).map(|_| format!("ggml-{id}.bin"))
}

/// Hugging Face resolve URL for a known model id, else `None`.
fn model_url(id: &str) -> Option<String> {
    model_filename(id).map(|f| format!("{HF_BASE}/{f}"))
}

fn model_sha256(id: &str) -> Option<&'static str> {
    match id {
        "small" => Some("1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"),
        "medium" => Some("6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208"),
        "large-v3-turbo" => Some("1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"),
        "large-v3" => Some("64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2"),
        _ => None,
    }
}

/// Encode mono/stereo 16-bit PCM as a canonical 44-byte-header WAV (what Whisper
/// wants). Kept in memory — the buffer never touches disk (ADR-001).
fn wav_from_pcm16(samples: &[i16], sample_rate: u32, channels: u16) -> Vec<u8> {
    let bits: u16 = 16;
    let byte_rate = sample_rate * channels as u32 * (bits / 8) as u32;
    let block_align = channels * (bits / 8);
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    v.extend_from_slice(&1u16.to_le_bytes()); // audio format = PCM
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&sample_rate.to_le_bytes());
    v.extend_from_slice(&byte_rate.to_le_bytes());
    v.extend_from_slice(&block_align.to_le_bytes());
    v.extend_from_slice(&bits.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

/// Build a `multipart/form-data` body: the WAV file part + simple string fields.
/// Pure → unit-tested. The caller sets `Content-Type: multipart/form-data; boundary=<b>`.
fn multipart_body(boundary: &str, wav: &[u8], fields: &[(&str, &str)]) -> Vec<u8> {
    let mut b = Vec::new();
    for (k, v) in fields {
        b.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        b.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{k}\"\r\n\r\n").as_bytes(),
        );
        b.extend_from_slice(v.as_bytes());
        b.extend_from_slice(b"\r\n");
    }
    b.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    b.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n",
    );
    b.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");
    b.extend_from_slice(wav);
    b.extend_from_slice(b"\r\n");
    b.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    b
}

/// whisper-server `/inference` URL for a port.
fn inference_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/inference")
}

/// whisper-server startup args. Anti-hallucination is applied **per request**:
/// `temperature_inc=0` disables whisper's temperature fallback ladder (= whisper-cli
/// `--no-fallback`), and each `/inference` is an independent, stateless call so no
/// prior transcript conditions the next one (= whisper-cli `--max-context 0`). Those
/// literal CLI flags are not whisper-server flags and aren't needed here (ADR-007).
fn server_args(model: &Path, vad_model: &Path, port: u16, threads: usize) -> Vec<String> {
    vec![
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "-t".into(),
        threads.to_string(),
        // Server-side Silero VAD (whisper.cpp built-in): only detected speech reaches
        // Whisper, so silence / non-speech can't become hallucinated text — the warm,
        // resident equivalent of the planned per-frame VAD gate (ADR-007).
        "--vad".into(),
        "--vad-model".into(),
        vad_model.to_string_lossy().into_owned(),
    ]
}

/// Per-request `/inference` fields enforcing deterministic, faithful decoding.
/// `prompt` is the custom-dictionary bias text (Whisper's initial prompt) — a
/// comma-joined term list that nudges spelling of names/jargon (custom-dictionary.md).
fn inference_fields(language: Option<&str>, prompt: Option<&str>) -> Vec<(String, String)> {
    let mut f = vec![
        ("temperature".into(), "0.0".into()),
        // temperature_inc=0 disables whisper's temperature fallback ladder (the
        // whisper-server equivalent of whisper-cli's `--no-fallback`).
        ("temperature_inc".into(), "0.0".into()),
        ("response_format".into(), "json".into()),
    ];
    if let Some(lang) = language {
        f.push(("language".into(), lang.to_string()));
    }
    if let Some(p) = prompt.map(str::trim).filter(|p| !p.is_empty()) {
        f.push(("prompt".into(), p.to_string()));
    }
    f
}

/// Extract the transcript from a whisper-server JSON reply (`{"text": "..."}`);
/// falls back to the raw body if it isn't the expected JSON.
fn parse_inference_text(body: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) => v
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .trim()
            .to_string(),
        Err(_) => body.trim().to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Filesystem / model management (reused from Toolzy)
// ─────────────────────────────────────────────────────────────────────────────

fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("models"))
}

fn require_model(app: &AppHandle, model: &str) -> Result<PathBuf, String> {
    let filename = model_filename(model).ok_or_else(|| format!("unknown model: {model}"))?;
    let path = models_dir(app)?.join(filename);
    if path.exists() {
        if let Some(expected) = model_sha256(model) {
            verify_file_sha256(&path, expected)?;
        }
        Ok(path)
    } else {
        Err(format!("model not downloaded: {model}"))
    }
}

fn require_vad(app: &AppHandle) -> Result<PathBuf, String> {
    let path = models_dir(app)?.join(VAD_FILENAME);
    if path.exists() {
        verify_file_sha256(&path, VAD_SHA256)?;
        Ok(path)
    } else {
        Err("model not downloaded: silero VAD".into())
    }
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_file_sha256(path: &Path, expected: &str) -> Result<(), String> {
    let got = file_sha256(path)?;
    if got.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!(
            "integrity check failed for {}: expected {expected}, got {got}",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("download")
        ))
    }
}

fn file_matches_size(path: &Path, expected: u64) -> bool {
    std::fs::metadata(path).map(|m| m.len() == expected).unwrap_or(false)
}

fn valid_downloaded_model(dir: &Path, id: &str) -> bool {
    let Some(model) = model_def(id) else {
        return false;
    };
    file_matches_size(&dir.join(format!("ggml-{}.bin", model.id)), model.size_bytes)
        && file_matches_size(&dir.join(VAD_FILENAME), VAD_SIZE_BYTES)
}

fn remove_invalid_file(path: &Path, expected: &str) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    if verify_file_sha256(path, expected).is_ok() {
        return Ok(true);
    }
    std::fs::remove_file(path)
        .map_err(|e| format!("invalid download could not be replaced: {e}"))?;
    Ok(false)
}

/// Stream a URL to `dest` via a `.part` file renamed on completion, so an
/// interrupted download leaves no half file. Blocking — run off the async runtime.
/// (Lifted from Toolzy's `transcription.rs`, ureq 3.)
///
/// SECURITY (deep-review, owner-gated): downloads carry NO content-integrity check.
/// TLS protects transit, but a compromised HF/GitHub mirror or a CA-trusted MITM could
/// substitute a tampered file. This is low-risk for the model `.bin` (data) but higher
/// for the CUDA engine zip (executable DLLs loaded in-process — see `install_gpu_engine`).
/// HARDENING: pin an expected SHA-256 per artifact (from the upstream whisper.cpp release
/// notes) and verify the `.part` before the final rename, rejecting on mismatch. Deferred
/// because it needs the owner to supply/maintain the trusted hashes.
fn download_file(
    url: &str,
    dest: &Path,
    progress: Option<&Channel<DownloadProgress>>,
    expected_sha256: &str,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let resp = ureq::get(url).call().map_err(|e| format!("download failed: {e}"))?;
    let total: Option<u64> = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    let part = dest.with_extension("part");
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut reader = resp.into_body().into_reader();
    let mut buf = [0u8; 65536];
    let mut downloaded = 0u64;

    loop {
        if cancelled.load(Ordering::Relaxed) {
            let _ = std::fs::remove_file(&part);
            return Err("download cancelled".into());
        }
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if cancelled.load(Ordering::Relaxed) {
            let _ = std::fs::remove_file(&part);
            return Err("download cancelled".into());
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        downloaded += n as u64;
        if let Some(ch) = progress {
            let percent = match total {
                Some(t) if t > 0 => downloaded as f64 / t as f64 * 100.0,
                _ => 0.0,
            };
            let _ = ch.send(DownloadProgress { percent, downloaded, total });
        }
    }

    drop(file);
    if cancelled.load(Ordering::Relaxed) {
        let _ = std::fs::remove_file(&part);
        return Err("download cancelled".into());
    }
    verify_file_sha256(&part, expected_sha256)?;
    std::fs::rename(&part, dest).map_err(|e| e.to_string())
}

/// List the offered models, flagging which are installed (drives the picker + gate).
#[tauri::command]
pub fn list_whisper_models(app: AppHandle) -> Result<Vec<WhisperModel>, String> {
    let dir = models_dir(&app)?;
    Ok(MODELS
        .iter()
        .map(|m| WhisperModel {
            id: m.id.into(),
            label: m.label.into(),
            size_mb: m.size_mb,
            downloaded: valid_downloaded_model(&dir, m.id),
            recommended: m.recommended,
        })
        .collect())
}

/// Download a model (and the small Silero VAD model once) to app-data, streaming
/// progress. Reuses an installed model. Returns the model path.
#[tauri::command]
pub async fn download_whisper_model(
    app: AppHandle,
    stt: State<'_, SttState>,
    model: String,
    on_progress: Channel<DownloadProgress>,
) -> Result<String, String> {
    let url = model_url(&model).ok_or_else(|| format!("unknown model: {model}"))?;
    let expected_sha256 =
        model_sha256(&model).ok_or_else(|| format!("missing trusted hash for model: {model}"))?;
    let filename = model_filename(&model).ok_or_else(|| format!("unknown model: {model}"))?;
    let dir = models_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let dest = dir.join(&filename);
    let vad_path = dir.join(VAD_FILENAME);
    let dest_str = dest.to_string_lossy().into_owned();
    let cancelled = {
        let mut downloads = stt
            .downloads
            .lock()
            .map_err(|_| "stt download state poisoned".to_string())?;
        if downloads.contains_key(&model) {
            return Err(format!("download already running: {model}"));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        downloads.insert(model.clone(), cancelled.clone());
        cancelled
    };
    let cleanup_model = model.clone();
    let task_cancelled = cancelled.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let vad_ready = remove_invalid_file(&vad_path, VAD_SHA256)?;
        let model_ready = remove_invalid_file(&dest, expected_sha256)?;
        if !vad_ready {
            download_file(VAD_URL, &vad_path, None, VAD_SHA256, &task_cancelled)?;
        }
        if !model_ready {
            download_file(
                &url,
                &dest,
                Some(&on_progress),
                expected_sha256,
                &task_cancelled,
            )?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);

    if let Ok(mut downloads) = stt.downloads.lock() {
        downloads.remove(&cleanup_model);
    }

    result?;

    Ok(dest_str)
}

#[tauri::command]
pub fn cancel_whisper_model_download(
    stt: State<'_, SttState>,
    model: String,
) -> Result<(), String> {
    let downloads = stt
        .downloads
        .lock()
        .map_err(|_| "stt download state poisoned".to_string())?;
    let Some(cancelled) = downloads.get(&model) else {
        return Ok(());
    };
    cancelled.store(true, Ordering::Relaxed);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// GPU engine (optional, NVIDIA) — reused from Toolzy
// ─────────────────────────────────────────────────────────────────────────────

fn gpu_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("engine-cuda"))
}

/// True when an NVIDIA driver is present (nvcuda.dll) — the GPU engine only helps then.
fn nvidia_present() -> bool {
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        Path::new(&root).join("System32").join("nvcuda.dll").exists()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(hit) = find_file(&p, name) {
                return Some(hit);
            }
        } else if p.file_name().and_then(|n| n.to_str()) == Some(name) {
            return Some(p);
        }
    }
    None
}

fn extract_zip(zip: &Path, tmp: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(tmp);
    std::fs::create_dir_all(tmp).map_err(|e| e.to_string())?;
    let ok = std::process::Command::new("tar")
        .args(["-xf", &zip.to_string_lossy(), "-C", &tmp.to_string_lossy()])
        .status()
        .map_err(|e| format!("extract failed: {e}"))?
        .success();
    if ok {
        Ok(())
    } else {
        Err("extract failed".into())
    }
}

/// Copy the GPU build's `whisper-server.exe` + sibling DLLs (located by finding the
/// server exe) into `dest`.
fn copy_engine_files(extracted: &Path, dest: &Path) -> Result<(), String> {
    let exe = find_file(extracted, "whisper-server.exe")
        .ok_or("whisper-server.exe not in archive")?;
    let src = exe.parent().ok_or("bad archive layout")?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name();
        let keep = name.to_str().is_some_and(|n| {
            n == "whisper-server.exe" || n.to_ascii_lowercase().ends_with(".dll")
        });
        if keep {
            std::fs::copy(entry.path(), dest.join(&name)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn install_gpu_engine(dir: &Path, on_progress: &Channel<DownloadProgress>) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let zip = dir.join("engine.zip");
    let cancelled = AtomicBool::new(false);
    download_file(GPU_URL, &zip, Some(on_progress), GPU_SHA256, &cancelled)?;
    let tmp = dir.join("extract");
    extract_zip(&zip, &tmp)?;
    copy_engine_files(&tmp, dir)?;
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_file(&zip);
    Ok(())
}

/// Whether an NVIDIA GPU is present and whether the GPU engine is installed.
#[tauri::command]
pub fn gpu_engine_status(app: AppHandle) -> Result<GpuStatus, String> {
    Ok(GpuStatus {
        gpu_present: nvidia_present(),
        downloaded: gpu_dir(&app)?.join("whisper-server.exe").exists(),
    })
}

/// Download the self-contained NVIDIA (CUDA) engine on demand, streaming progress.
#[tauri::command]
pub async fn download_gpu_engine(
    app: AppHandle,
    stt: State<'_, SttState>,
    on_progress: Channel<DownloadProgress>,
) -> Result<(), String> {
    if gpu_dir(&app)?.join("whisper-server.exe").exists() {
        return Ok(());
    }
    let dir = gpu_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || install_gpu_engine(&dir, &on_progress))
        .await
        .map_err(|e| e.to_string())??;
    // Drop any warm CPU server so the next dictation re-warms on the GPU build
    // (server_exe now prefers engine-cuda/whisper-server.exe).
    let _ = unload(&stt);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Warm server lifecycle + transcription
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve the whisper-server executable: the CUDA build if installed, else the
/// bundled CPU build (resource dir in a bundle, `binaries/` beside src-tauri in dev).
fn server_exe(app: &AppHandle) -> Result<PathBuf, String> {
    let gpu = gpu_dir(app)?.join("whisper-server.exe");
    if gpu.exists() {
        return Ok(gpu);
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        candidates.push(res.join("binaries").join("whisper-server.exe"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            candidates.push(d.join("whisper-server.exe"));
            candidates.push(d.join("binaries").join("whisper-server.exe"));
        }
    }
    // dev: src-tauri/binaries
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join("whisper-server.exe"),
    );
    candidates
        .into_iter()
        .find(|p| p.exists())
        .ok_or_else(|| "whisper-server not found (run scripts/fetch-binaries.mjs)".to_string())
}

/// Pick a free localhost port by binding an ephemeral one and releasing it.
fn free_port() -> Result<u16, String> {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    Ok(port)
}

/// Block until the server accepts TCP connections on `port`, or time out.
fn wait_for_server(port: u16, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Err("whisper-server did not become ready in time".into())
}

fn spawn_server(exe: &Path, args: &[String]) -> Result<Child, String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW — no console flash
    }
    cmd.spawn().map_err(|e| format!("failed to start whisper-server: {e}"))
}

/// Load a model into a warm whisper-server (idempotent if already warm with the
/// same model). Spawns the server and waits for it to accept connections.
pub fn warm_model(app: &AppHandle, state: &SttState, model: &str) -> Result<(), String> {
    {
        let guard = state.server.lock().map_err(|_| "stt state poisoned".to_string())?;
        if let Some(s) = guard.as_ref() {
            if s.model == model {
                return Ok(()); // already warm with this model
            }
        }
    }
    let model_path = require_model(app, model)?;
    // Silero VAD must be present too (downloaded alongside the model) — it's passed to
    // the server so recognition is VAD-gated (anti-hallucination).
    let vad_path = require_vad(app)?;
    let exe = server_exe(app)?;
    let gpu = exe.starts_with(gpu_dir(app)?);
    let port = free_port()?;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    {
        let mut guard = state.server.lock().map_err(|_| "stt state poisoned".to_string())?;
        if guard.as_ref().is_some_and(|s| s.model != model) {
            *guard = None;
        }
    }
    let child = spawn_server(&exe, &server_args(&model_path, &vad_path, port, threads))?;
    wait_for_server(port, Duration::from_secs(60))?;

    let mut guard = state.server.lock().map_err(|_| "stt state poisoned".to_string())?;
    *guard = Some(WarmServer { child, port, model: model.to_string(), gpu });
    Ok(())
}

/// Transcribe one endpointed utterance (16 kHz mono f32 PCM) against the warm
/// server. In-process hot path — called by the dictation orchestrator, NOT a
/// command, so an utterance never round-trips through the webview.
pub fn transcribe_chunk(
    state: &SttState,
    samples: &[f32],
    language: Option<&str>,
    prompt: Option<&str>,
) -> Result<String, String> {
    let port = {
        let guard = state.server.lock().map_err(|_| "stt state poisoned".to_string())?;
        guard.as_ref().ok_or("no model warm")?.port
    };
    // Single canonical rounding quantizer (audio.rs) — no truncating duplicate here.
    let pcm: Vec<i16> = samples.iter().map(|&x| crate::audio::f32_to_s16(x)).collect();
    let wav = wav_from_pcm16(&pcm, 16_000, 1);
    let boundary = "----miaformboundary";
    let fields_owned = inference_fields(language, prompt);
    let fields: Vec<(&str, &str)> =
        fields_owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let body = multipart_body(boundary, &wav, &fields);

    let resp = ureq::post(inference_url(port))
        .header("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
        .send(body.as_slice())
        .map_err(|e| format!("whisper engine failed: {e}"))?;
    let mut text = String::new();
    resp.into_body()
        .into_reader()
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(parse_inference_text(&text))
}

/// Stop the warm server and free its RAM (also runs on app exit via `Drop`).
pub fn unload(state: &SttState) -> Result<(), String> {
    let mut guard = state.server.lock().map_err(|_| "stt state poisoned".to_string())?;
    *guard = None; // Drop kills the child
    Ok(())
}

/// Current warm-engine status for the UI.
#[tauri::command]
pub fn warm_status(state: tauri::State<'_, SttState>) -> Result<WarmStatus, String> {
    let guard = state.server.lock().map_err(|_| "stt state poisoned".to_string())?;
    Ok(match guard.as_ref() {
        Some(s) => WarmStatus {
            loaded: true,
            model: Some(s.model.clone()),
            backend: "whisperServer".into(),
            gpu: s.gpu,
        },
        None => WarmStatus {
            loaded: false,
            model: None,
            backend: "whisperServer".into(),
            gpu: false,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_url_and_filename_known_vs_unknown() {
        assert_eq!(
            model_url("large-v3").unwrap(),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin"
        );
        assert_eq!(model_filename("small").unwrap(), "ggml-small.bin");
        assert_eq!(
            model_sha256("small").unwrap(),
            "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"
        );
        assert!(model_url("nope").is_none());
        assert!(model_filename("nope").is_none());
        assert!(model_sha256("nope").is_none());
    }

    // NOTE: the f32→i16 quantizer is now the single canonical `audio::f32_to_s16`
    // (rounding, clamped) — its clamp/round behavior is covered in audio.rs tests.

    #[test]
    fn remove_invalid_file_keeps_valid_and_deletes_mismatch() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("mia-stt-{nonce}"));
        std::fs::create_dir_all(&dir).unwrap();

        let valid = dir.join("valid.bin");
        let invalid = dir.join("invalid.bin");
        let abc_sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        std::fs::write(&valid, b"abc").unwrap();
        std::fs::write(&invalid, b"bad").unwrap();

        assert!(remove_invalid_file(&valid, abc_sha).unwrap());
        assert!(valid.exists());
        assert!(!remove_invalid_file(&invalid, abc_sha).unwrap());
        assert!(!invalid.exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn file_matches_size_checks_exact_length() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("mia-stt-size-{nonce}"));
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("model.bin");
        std::fs::write(&path, b"abc").unwrap();

        assert!(file_matches_size(&path, 3));
        assert!(!file_matches_size(&path, 2));
        assert!(!file_matches_size(&dir.join("missing.bin"), 3));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn wav_header_is_canonical() {
        let wav = wav_from_pcm16(&[0, 1, -1], 16_000, 1);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        // 44-byte header + 3 samples * 2 bytes
        assert_eq!(wav.len(), 44 + 6);
        // sample rate at offset 24 (LE)
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), 16_000);
        // channels at offset 22
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
        // data length at offset 40
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 6);
    }

    #[test]
    fn multipart_structure() {
        let body = multipart_body("BND", b"\x00\x01", &[("temperature", "0.0")]);
        let s = String::from_utf8_lossy(&body);
        assert!(s.contains("--BND\r\n"));
        assert!(s.contains("name=\"temperature\""));
        assert!(s.contains("name=\"file\"; filename=\"audio.wav\""));
        assert!(s.contains("Content-Type: audio/wav"));
        assert!(s.trim_end().ends_with("--BND--"));
    }

    #[test]
    fn inference_url_format() {
        assert_eq!(inference_url(8765), "http://127.0.0.1:8765/inference");
    }

    #[test]
    fn server_args_has_model_host_port_threads() {
        let a = server_args(Path::new("m.bin"), Path::new("vad.bin"), 8765, 8);
        assert!(a.windows(2).any(|w| w[0] == "-m" && w[1] == "m.bin"));
        assert!(a.windows(2).any(|w| w[0] == "--host" && w[1] == "127.0.0.1"));
        assert!(a.windows(2).any(|w| w[0] == "--port" && w[1] == "8765"));
        assert!(a.windows(2).any(|w| w[0] == "-t" && w[1] == "8"));
    }

    #[test]
    fn server_args_enable_vad_with_model() {
        let a = server_args(Path::new("m.bin"), Path::new("vad.bin"), 8765, 8);
        assert!(a.iter().any(|x| x == "--vad"));
        assert!(a.windows(2).any(|w| w[0] == "--vad-model" && w[1] == "vad.bin"));
    }

    #[test]
    fn inference_fields_enforce_determinism() {
        let f = inference_fields(Some("pt"), None);
        assert!(f.iter().any(|(k, v)| k == "temperature" && v == "0.0"));
        assert!(f.iter().any(|(k, v)| k == "temperature_inc" && v == "0.0"));
        assert!(f.iter().any(|(k, v)| k == "language" && v == "pt"));
        // no language field when None
        assert!(!inference_fields(None, None).iter().any(|(k, _)| k == "language"));
    }

    #[test]
    fn inference_fields_carry_bias_prompt_when_present() {
        let f = inference_fields(Some("pt"), Some("MIA, Tauri"));
        assert!(f.iter().any(|(k, v)| k == "prompt" && v == "MIA, Tauri"));
        // blank/whitespace bias is dropped, not sent empty
        assert!(!inference_fields(None, Some("   ")).iter().any(|(k, _)| k == "prompt"));
        assert!(!inference_fields(None, None).iter().any(|(k, _)| k == "prompt"));
    }

    #[test]
    fn parse_inference_json_and_fallback() {
        assert_eq!(parse_inference_text(r#"{"text":"  olá mundo  "}"#), "olá mundo");
        assert_eq!(parse_inference_text("plain text body"), "plain text body");
        assert_eq!(parse_inference_text(r#"{"other":1}"#), "");
    }
}
