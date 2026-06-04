import { type Channel, invoke } from "@tauri-apps/api/core";

/** A Whisper model offered in the picker; mirrors Rust `stt::WhisperModel`. */
export interface WhisperModel {
  id: string;
  label: string;
  sizeMb: number;
  downloaded: boolean;
  recommended: boolean;
}

/** NVIDIA GPU presence + CUDA-engine install state; mirrors Rust `stt::GpuStatus`. */
export interface GpuStatus {
  gpuPresent: boolean;
  downloaded: boolean;
}

/** Warm-engine status for the UI; mirrors Rust `stt::WarmStatus`. */
export interface WarmStatus {
  loaded: boolean;
  model: string | null;
  warming: boolean;
  targetModel: string | null;
  backend: string;
  gpu: boolean;
}

/** One-time download progress streamed over a Tauri channel; mirrors `stt::DownloadProgress`. */
export interface DownloadProgress {
  percent: number;
  downloaded: number;
  total: number | null;
}

/** List the offered Whisper models, flagging which are installed (drives the download gate). */
export function listWhisperModels(): Promise<WhisperModel[]> {
  return invoke<WhisperModel[]>("list_whisper_models");
}

/** Download a model (and the Silero VAD once) to app-data; resolves to the model path. */
export function downloadWhisperModel(
  model: string,
  onProgress: Channel<DownloadProgress>,
): Promise<string> {
  return invoke<string>("download_whisper_model", { model, onProgress });
}

/** Cancel an in-flight Whisper model download. */
export function cancelWhisperModelDownload(model: string): Promise<void> {
  return invoke<void>("cancel_whisper_model_download", { model });
}

/** Delete a downloaded Whisper model from app-data. */
export function deleteWhisperModel(model: string): Promise<void> {
  return invoke<void>("delete_whisper_model", { model });
}

/** Whether an NVIDIA GPU is present and whether the CUDA engine is already installed. */
export function gpuEngineStatus(): Promise<GpuStatus> {
  return invoke<GpuStatus>("gpu_engine_status");
}

/** Download the optional NVIDIA (CUDA) engine on demand, streaming progress. */
export function downloadGpuEngine(onProgress: Channel<DownloadProgress>): Promise<void> {
  return invoke<void>("download_gpu_engine", { onProgress });
}

/** Current warm-engine status (loaded model + backend + GPU flag). */
export function warmStatus(): Promise<WarmStatus> {
  return invoke<WarmStatus>("warm_status");
}
