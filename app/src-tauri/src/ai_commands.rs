//! AI Command Mode + Polish — optional, opt-in local-LLM text intelligence
//! (ADR-008, Phase 2). The LLM never touches the faithful default path; it runs
//! only when the user explicitly asks (a command/polish). See
//! `docs/specs/ai-commands.md`.
//!
//! This file is the **pure, cargo-tested core**: the cheap pre-LLM `route_intent`
//! classifier (no model, sub-ms), the GBNF `command_grammar` that constrains
//! Command-Mode decoding to a valid `ParsedCommand`, the per-action/per-language
//! `build_prompt`, and `validate_parsed`. The llama.cpp runtime, model download,
//! and the `run_command`/`polish` commands are the runtime-pending follow-up.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::cleanup::Lang;
use crate::stt::{download_file, DownloadProgress};

/// Router output (Rule 2). Default-conservative: anything unclear → `Dictation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Intent {
    Dictation,
    Command,
    Polish,
}

/// The constrained set of transforms Command Mode may emit (Rule 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandAction {
    Concise,
    Formal,
    Casual,
    BulletList,
    Summarize,
    Translate,
    Fix,
    Expand,
    Custom,
}

/// What the command operates on (Rule 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandTarget {
    LastInserted,
    Selection,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommandParams {
    pub lang: Option<String>,
    pub instruction: Option<String>,
}

/// The constrained Command envelope the grammar forces the model to emit (Rule 4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedCommand {
    pub action: CommandAction,
    pub target: CommandTarget,
    pub params: CommandParams,
}

/// Routing configuration (subset of the AI settings). Opt-in: command mode off by
/// default, so `route_intent` is a no-op until the user enables it (Rule 1).
#[derive(Clone, Debug)]
pub struct AiConfig {
    pub command_mode_enabled: bool,
    pub polish_enabled: bool,
    pub trigger_prefix: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self { command_mode_enabled: false, polish_enabled: false, trigger_prefix: "hey mia".into() }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Trigger-phrase tables (per language; Other falls back to English)
// ─────────────────────────────────────────────────────────────────────────────

fn command_verbs(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::PtBr => &[
            "faça", "faca", "deixe", "reescreva", "transforme", "resuma", "traduza", "corrija",
            "expanda", "encurte", "refaça", "refaca", "liste",
        ],
        _ => &[
            "make", "rewrite", "turn", "summarize", "translate", "fix", "correct", "expand",
            "shorten", "rephrase", "list",
        ],
    }
}

fn target_refs(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::PtBr => &["isso", "isto", "o texto", "esse texto", "esta frase"],
        _ => &["this", "that", "the text", " it"],
    }
}

fn polish_triggers(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::PtBr => &["melhore isso", "melhore isto", "revise isso", "poli isso"],
        _ => &["polish this", "polish that", "clean this up", "improve this"],
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure core
// ─────────────────────────────────────────────────────────────────────────────

/// Classify an utterance **before** any LLM cost (Rule 2). Conservative: only a
/// clear trigger prefix, polish phrase, or imperative "verb … this/text" routes
/// away from `Dictation`. Disabled command mode → always `Dictation` (Rule 1).
pub fn route_intent(transcript: &str, lang: Lang, cfg: &AiConfig) -> Intent {
    if !cfg.command_mode_enabled {
        return Intent::Dictation;
    }
    let t = transcript.trim().to_lowercase();
    if t.is_empty() {
        return Intent::Dictation;
    }
    let prefix = cfg.trigger_prefix.trim().to_lowercase();
    if !prefix.is_empty() && t.starts_with(&prefix) {
        return Intent::Command;
    }
    if cfg.polish_enabled && polish_triggers(lang).iter().any(|p| t.starts_with(p)) {
        return Intent::Polish;
    }
    if starts_with_command_verb(&t, lang) && mentions_target(&t, lang) {
        return Intent::Command;
    }
    Intent::Dictation
}

fn starts_with_command_verb(lower: &str, lang: Lang) -> bool {
    let first = lower.split_whitespace().next().unwrap_or("");
    command_verbs(lang).contains(&first)
}

fn mentions_target(lower: &str, lang: Lang) -> bool {
    target_refs(lang).iter().any(|r| lower.contains(r))
}

/// The GBNF grammar constraining Command-Mode decoding to a valid `ParsedCommand`
/// JSON (Rule 4). Static — llama.cpp loads it verbatim.
pub fn command_grammar() -> &'static str {
    r#"root   ::= "{" ws "\"action\"" ws ":" ws action ws "," ws "\"target\"" ws ":" ws target ws "," ws "\"params\"" ws ":" ws params ws "}"
action ::= "\"concise\"" | "\"formal\"" | "\"casual\"" | "\"bulletList\"" | "\"summarize\"" | "\"translate\"" | "\"fix\"" | "\"expand\"" | "\"custom\""
target ::= "\"lastInserted\"" | "\"selection\""
params ::= "{" ws ( "\"lang\"" ws ":" ws string ( ws "," ws "\"instruction\"" ws ":" ws string )? | "\"instruction\"" ws ":" ws string )? ws "}"
string ::= "\"" ([^"\\] | "\\" .)* "\""
ws     ::= [ \t\n]*"#
}

/// Compose the per-action, per-language instruction prompt over `target` (Rule 4/6).
/// `instr` carries the free-text for `Custom`.
pub fn build_prompt(action: CommandAction, lang: Lang, target: &str, instr: Option<&str>) -> String {
    let directive = action_directive(action, lang, instr);
    let (sys, label) = match lang {
        Lang::PtBr => (
            "Você edita texto fielmente. Não invente fatos. Responda só com o texto transformado.",
            "Texto",
        ),
        _ => (
            "You edit text faithfully. Do not invent facts. Reply with only the transformed text.",
            "Text",
        ),
    };
    format!("{sys}\n\n{directive}\n\n{label}:\n{target}")
}

fn action_directive(action: CommandAction, lang: Lang, instr: Option<&str>) -> String {
    let pt = lang == Lang::PtBr;
    match action {
        CommandAction::Concise => if pt { "Torne o texto mais conciso." } else { "Make the text more concise." }.to_string(),
        CommandAction::Formal => if pt { "Reescreva em tom formal." } else { "Rewrite in a formal tone." }.to_string(),
        CommandAction::Casual => if pt { "Reescreva em tom casual." } else { "Rewrite in a casual tone." }.to_string(),
        CommandAction::BulletList => if pt { "Converta em lista de tópicos." } else { "Convert into a bullet list." }.to_string(),
        CommandAction::Summarize => if pt { "Resuma o texto." } else { "Summarize the text." }.to_string(),
        CommandAction::Translate => if pt { "Traduza o texto." } else { "Translate the text." }.to_string(),
        CommandAction::Fix => if pt { "Corrija gramática e ortografia." } else { "Fix grammar and spelling." }.to_string(),
        CommandAction::Expand => if pt { "Expanda o texto com mais detalhe." } else { "Expand the text with more detail." }.to_string(),
        CommandAction::Custom => instr
            .map(|s| s.to_string())
            .unwrap_or_else(|| if pt { "Aplique a instrução do usuário." } else { "Apply the user's instruction." }.to_string()),
    }
}

/// Reject a parsed command the model shouldn't have produced (Rule 7): `Translate`
/// needs a `lang`, `Custom` needs an `instruction`.
pub fn validate_parsed(cmd: &ParsedCommand) -> Result<(), String> {
    match cmd.action {
        CommandAction::Translate if cmd.params.lang.as_deref().unwrap_or("").trim().is_empty() => {
            Err("command not understood".to_string())
        }
        CommandAction::Custom if cmd.params.instruction.as_deref().unwrap_or("").trim().is_empty() => {
            Err("command not understood".to_string())
        }
        _ => Ok(()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Runtime: on-demand GGUF download + warm llama-server sidecar + commands.
//
// Mirrors the warm-whisper-server pattern (stt.rs, ADR-004): the LLM is a resident
// llama-server (llama.cpp, cmake-free) loaded once on demand, never bundled, and runs
// ONLY for explicit Command/Polish actions — never on the faithful dictation path
// (ADR-008). Models are fetched on demand from Hugging Face (ADR-007).
// ─────────────────────────────────────────────────────────────────────────────

/// One offered local LLM (GGUF on Hugging Face). Q4_K_M ≈ 1.5-2 GB RAM.
struct LlmDef {
    id: &'static str,
    label: &'static str,
    repo: &'static str,
    file: &'static str,
    size_mb: u32,
}

const LLMS: &[LlmDef] = &[
    LlmDef {
        id: "qwen2.5-3b",
        label: "Qwen2.5 3B Instruct",
        repo: "bartowski/Qwen2.5-3B-Instruct-GGUF",
        file: "Qwen2.5-3B-Instruct-Q4_K_M.gguf",
        size_mb: 1930,
    },
    LlmDef {
        id: "llama-3.2-3b",
        label: "Llama 3.2 3B Instruct",
        repo: "bartowski/Llama-3.2-3B-Instruct-GGUF",
        file: "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        size_mb: 2020,
    },
];

fn llm_def(id: &str) -> Option<&'static LlmDef> {
    LLMS.iter().find(|m| m.id == id)
}

fn llm_url(def: &LlmDef) -> String {
    format!("https://huggingface.co/{}/resolve/main/{}", def.repo, def.file)
}

fn llm_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("llm"))
}

/// Map the persisted `AiModel` setting to a registry id.
fn ai_model_id(model: crate::settings::AiModel) -> &'static str {
    match model {
        crate::settings::AiModel::Qwen25_3b => "qwen2.5-3b",
        crate::settings::AiModel::Llama32_3b => "llama-3.2-3b",
    }
}

fn lang_from_code(code: &str) -> Lang {
    match code {
        "pt" => Lang::PtBr,
        "en" => Lang::En,
        _ => Lang::Other,
    }
}

/// One offered model for the Settings picker; mirrors `stt::WhisperModel`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmModel {
    id: String,
    label: String,
    size_mb: u32,
    downloaded: bool,
}

/// AI feature + model status for Settings + gating.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    enabled: bool,
    model_installed: bool,
    model_id: String,
    loaded: bool,
    backend: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    new_text: String,
    action: CommandAction,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolishResult {
    polished_text: String,
}

struct WarmLlm {
    child: std::process::Child,
    port: u16,
    model: String,
}

impl Drop for WarmLlm {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// Managed Tauri state: the warm llama-server (loaded once, freed on demand).
#[derive(Default)]
pub struct LlmState {
    server: Mutex<Option<WarmLlm>>,
}

/// Resolve the llama-server executable. It lives in a `binaries/llama/` subdir (not
/// alongside whisper-server) because llama.cpp and whisper.cpp ship same-named
/// `ggml*.dll` that would otherwise collide; co-locating each server with its own DLLs
/// keeps Windows' exe-dir DLL search correct.
fn llama_server_exe(app: &AppHandle) -> Result<PathBuf, String> {
    let rel = ["binaries", "llama", "llama-server.exe"];
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        candidates.push(rel.iter().fold(res, |p, s| p.join(s)));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            candidates.push(d.join("llama").join("llama-server.exe"));
            candidates.push(rel.iter().fold(d.to_path_buf(), |p, s| p.join(s)));
        }
    }
    candidates.push(rel.iter().fold(PathBuf::from(env!("CARGO_MANIFEST_DIR")), |p, s| p.join(s)));
    candidates
        .into_iter()
        .find(|p| p.exists())
        .ok_or_else(|| "llama-server not found (run scripts/fetch-binaries.mjs)".to_string())
}

fn llama_args(model: &Path, port: u16, threads: usize) -> Vec<String> {
    vec![
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "-t".into(),
        threads.to_string(),
        "-c".into(),
        "4096".into(),
    ]
}

fn spawn_llama(exe: &Path, args: &[String]) -> Result<std::process::Child, String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW — no console flash
    }
    cmd.spawn().map_err(|e| format!("model load failed: {e}"))
}

/// Load `model` into a warm llama-server (idempotent if already warm). Returns the port.
fn warm_llm(app: &AppHandle, state: &LlmState, model: &str) -> Result<u16, String> {
    {
        let guard = state.server.lock().map_err(|_| "llm state poisoned".to_string())?;
        if let Some(s) = guard.as_ref() {
            if s.model == model {
                return Ok(s.port);
            }
        }
    }
    let def = llm_def(model).ok_or_else(|| format!("unknown model: {model}"))?;
    let path = llm_dir(app)?.join(def.file);
    if !path.exists() {
        return Err("model not installed".to_string());
    }
    let exe = llama_server_exe(app)?;
    let port = crate::stt::free_port()?;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let child = spawn_llama(&exe, &llama_args(&path, port, threads))?;
    crate::stt::wait_for_server(port, Duration::from_secs(120))?;
    let mut guard = state.server.lock().map_err(|_| "llm state poisoned".to_string())?;
    *guard = Some(WarmLlm { child, port, model: model.to_string() });
    Ok(port)
}

/// One faithful, deterministic `/completion` call. `grammar` (GBNF) forces the output
/// into the valid shape when present (Command-Mode parsing).
fn llama_complete(port: u16, prompt: &str, grammar: Option<&str>, n_predict: u32) -> Result<String, String> {
    let mut payload = serde_json::json!({
        "prompt": prompt,
        "n_predict": n_predict,
        "temperature": 0.0,
        "cache_prompt": true,
        "stream": false,
    });
    if let Some(g) = grammar {
        payload["grammar"] = serde_json::Value::String(g.to_string());
    }
    let body = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    let resp = ureq::post(format!("http://127.0.0.1:{port}/completion"))
        .header("Content-Type", "application/json")
        .send(body.as_bytes())
        .map_err(|e| format!("llm timeout: {e}"))?;
    let mut text = String::new();
    resp.into_body().into_reader().read_to_string(&mut text).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(v.get("content").and_then(|c| c.as_str()).unwrap_or("").trim().to_string())
}

/// Prompt that asks the model for the `ParsedCommand` JSON; paired with
/// `command_grammar()` so the output is forced into the valid envelope (Rule 4).
fn command_parse_prompt(transcript: &str, lang: Lang) -> String {
    let intro = match lang {
        Lang::PtBr => "Converta o comando de edição falado no JSON {action,target,params}.",
        _ => "Convert the spoken editing command into the {action,target,params} JSON.",
    };
    format!("{intro}\nComando: \"{transcript}\"\nJSON:")
}

fn polish_prompt(text: &str, lang: Lang) -> String {
    let (sys, label) = match lang {
        Lang::PtBr => (
            "Você revisa texto ditado: corrige gramática, pontuação e disfluências sem mudar o sentido nem inventar. Responda só com o texto revisado.",
            "Texto",
        ),
        _ => (
            "You proofread dictated text: fix grammar, punctuation and disfluencies without changing meaning or inventing. Reply with only the revised text.",
            "Text",
        ),
    };
    format!("{sys}\n\n{label}:\n{text}")
}

/// List the offered local LLMs, flagging which are installed (download gate).
#[tauri::command]
pub fn list_llm_models(app: AppHandle) -> Result<Vec<LlmModel>, String> {
    let dir = llm_dir(&app)?;
    Ok(LLMS
        .iter()
        .map(|m| LlmModel {
            id: m.id.into(),
            label: m.label.into(),
            size_mb: m.size_mb,
            downloaded: dir.join(m.file).exists(),
        })
        .collect())
}

/// AI feature + model status (drives Settings + gating).
#[tauri::command]
pub fn ai_status(
    app: AppHandle,
    settings: State<'_, crate::settings::SettingsState>,
    llm: State<'_, LlmState>,
) -> Result<AiStatus, String> {
    let s = settings.snapshot()?;
    let model_id = ai_model_id(s.ai.model);
    let installed = llm_def(model_id).is_some_and(|d| llm_dir(&app).map(|dir| dir.join(d.file).exists()).unwrap_or(false));
    let loaded = llm
        .server
        .lock()
        .map_err(|_| "llm state poisoned".to_string())?
        .as_ref()
        .map(|w| w.model == model_id)
        .unwrap_or(false);
    Ok(AiStatus {
        enabled: s.ai.enabled,
        model_installed: installed,
        model_id: model_id.to_string(),
        loaded,
        backend: "llamaServer".into(),
    })
}

/// Download a model GGUF on demand (HF, `.part` → rename, streamed progress).
#[tauri::command]
pub async fn download_llm(
    app: AppHandle,
    model: String,
    on_progress: Channel<DownloadProgress>,
) -> Result<(), String> {
    let def = llm_def(&model).ok_or_else(|| format!("unknown model: {model}"))?;
    let url = llm_url(def);
    let dir = llm_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(def.file);
    if dest.exists() {
        return Ok(());
    }
    tauri::async_runtime::spawn_blocking(move || download_file(&url, &dest, Some(&on_progress)))
        .await
        .map_err(|e| e.to_string())?
}

/// Free the warm LLM's RAM on demand.
#[tauri::command]
pub fn unload_llm(llm: State<'_, LlmState>) -> Result<(), String> {
    *llm.server.lock().map_err(|_| "llm state poisoned".to_string())? = None;
    Ok(())
}

/// Command Mode: parse the spoken `transcript` into a constrained `ParsedCommand`
/// (grammar) and apply it to `target` text, returning the transform. Lazily warms the
/// model. Opt-in (ADR-008): `Err("ai disabled")` when the feature is off.
#[tauri::command]
pub async fn run_command(
    app: AppHandle,
    transcript: String,
    target: String,
    lang: String,
) -> Result<CommandResult, String> {
    tauri::async_runtime::spawn_blocking(move || run_command_blocking(&app, &transcript, &target, &lang))
        .await
        .map_err(|e| e.to_string())?
}

fn run_command_blocking(app: &AppHandle, transcript: &str, target: &str, lang: &str) -> Result<CommandResult, String> {
    let s = app.state::<crate::settings::SettingsState>().snapshot()?;
    if !s.ai.enabled {
        return Err("ai disabled".to_string());
    }
    if target.trim().is_empty() {
        return Err("no target text".to_string());
    }
    let lang = lang_from_code(lang);
    let model_id = ai_model_id(s.ai.model);
    let llm = app.state::<LlmState>();
    let port = warm_llm(app, &llm, model_id)?;
    let parsed = llama_complete(port, &command_parse_prompt(transcript, lang), Some(command_grammar()), 128)?;
    let cmd: ParsedCommand =
        serde_json::from_str(&parsed).map_err(|_| "command not understood".to_string())?;
    validate_parsed(&cmd)?;
    let new_text =
        llama_complete(port, &build_prompt(cmd.action, lang, target, cmd.params.instruction.as_deref()), None, 512)?;
    if new_text.is_empty() {
        return Err("command not understood".to_string());
    }
    Ok(CommandResult { new_text, action: cmd.action })
}

/// Polish: repair-constrained rewrite of `text` (grammar, punctuation, disfluencies).
#[tauri::command]
pub async fn polish(app: AppHandle, text: String, lang: String) -> Result<PolishResult, String> {
    tauri::async_runtime::spawn_blocking(move || polish_blocking(&app, &text, &lang))
        .await
        .map_err(|e| e.to_string())?
}

fn polish_blocking(app: &AppHandle, text: &str, lang: &str) -> Result<PolishResult, String> {
    let s = app.state::<crate::settings::SettingsState>().snapshot()?;
    if !s.ai.enabled {
        return Err("ai disabled".to_string());
    }
    if text.trim().is_empty() {
        return Err("no target text".to_string());
    }
    let lang = lang_from_code(lang);
    let model_id = ai_model_id(s.ai.model);
    let llm = app.state::<LlmState>();
    let port = warm_llm(app, &llm, model_id)?;
    let polished = llama_complete(port, &polish_prompt(text, lang), None, 512)?;
    Ok(PolishResult { polished_text: if polished.is_empty() { text.to_string() } else { polished } })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled() -> AiConfig {
        AiConfig { command_mode_enabled: true, polish_enabled: true, trigger_prefix: "hey mia".into() }
    }

    #[test]
    fn disabled_always_dictation() {
        let cfg = AiConfig::default(); // command mode off
        assert_eq!(route_intent("summarize this", Lang::En, &cfg), Intent::Dictation);
        assert_eq!(route_intent("hey mia do something", Lang::En, &cfg), Intent::Dictation);
    }

    #[test]
    fn trigger_prefix_routes_to_command() {
        assert_eq!(route_intent("Hey MIA make this formal", Lang::En, &enabled()), Intent::Command);
    }

    #[test]
    fn polish_phrase_routes_to_polish() {
        assert_eq!(route_intent("polish this please", Lang::En, &enabled()), Intent::Polish);
        assert_eq!(route_intent("melhore isso aí", Lang::PtBr, &enabled()), Intent::Polish);
    }

    #[test]
    fn imperative_verb_plus_target_is_command() {
        assert_eq!(route_intent("summarize this", Lang::En, &enabled()), Intent::Command);
        assert_eq!(route_intent("make the text formal", Lang::En, &enabled()), Intent::Command);
        assert_eq!(route_intent("resuma o texto", Lang::PtBr, &enabled()), Intent::Command);
    }

    #[test]
    fn plain_dictation_stays_dictation() {
        // Conservative default: ordinary speech is never a command.
        assert_eq!(route_intent("the weather is really nice today", Lang::En, &enabled()), Intent::Dictation);
        assert_eq!(route_intent("eu fui à praia ontem", Lang::PtBr, &enabled()), Intent::Dictation);
        // A command verb without a target reference does not trigger.
        assert_eq!(route_intent("make me a coffee", Lang::En, &enabled()), Intent::Dictation);
    }

    #[test]
    fn grammar_lists_actions_and_targets() {
        let g = command_grammar();
        assert!(g.contains("summarize"));
        assert!(g.contains("bulletList"));
        assert!(g.contains("lastInserted"));
        assert!(g.contains("root"));
    }

    #[test]
    fn prompt_includes_target_and_directive() {
        let p = build_prompt(CommandAction::Summarize, Lang::En, "a long paragraph", None);
        assert!(p.contains("a long paragraph"));
        assert!(p.to_lowercase().contains("summarize"));
        let pt = build_prompt(CommandAction::Formal, Lang::PtBr, "oi", None);
        assert!(pt.contains("formal"));
        assert!(pt.contains("oi"));
    }

    #[test]
    fn custom_prompt_uses_instruction() {
        let p = build_prompt(CommandAction::Custom, Lang::En, "x", Some("translate to pirate"));
        assert!(p.contains("translate to pirate"));
    }

    #[test]
    fn validate_rejects_incomplete_commands() {
        let translate_no_lang = ParsedCommand {
            action: CommandAction::Translate,
            target: CommandTarget::LastInserted,
            params: CommandParams::default(),
        };
        assert!(validate_parsed(&translate_no_lang).is_err());

        let translate_ok = ParsedCommand {
            action: CommandAction::Translate,
            target: CommandTarget::LastInserted,
            params: CommandParams { lang: Some("en".into()), instruction: None },
        };
        assert!(validate_parsed(&translate_ok).is_ok());

        let concise = ParsedCommand {
            action: CommandAction::Concise,
            target: CommandTarget::Selection,
            params: CommandParams::default(),
        };
        assert!(validate_parsed(&concise).is_ok());
    }

    #[test]
    fn llm_registry_urls_ids_and_lang() {
        let q = llm_def("qwen2.5-3b").unwrap();
        assert_eq!(
            llm_url(q),
            "https://huggingface.co/bartowski/Qwen2.5-3B-Instruct-GGUF/resolve/main/Qwen2.5-3B-Instruct-Q4_K_M.gguf"
        );
        assert!(llm_def("nope").is_none());
        assert_eq!(ai_model_id(crate::settings::AiModel::Qwen25_3b), "qwen2.5-3b");
        assert_eq!(ai_model_id(crate::settings::AiModel::Llama32_3b), "llama-3.2-3b");
        assert_eq!(lang_from_code("pt"), Lang::PtBr);
        assert_eq!(lang_from_code("en"), Lang::En);
        assert_eq!(lang_from_code("zz"), Lang::Other);
    }
}
