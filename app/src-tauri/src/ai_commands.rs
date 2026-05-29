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

use serde::{Deserialize, Serialize};

use crate::cleanup::Lang;

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
}
