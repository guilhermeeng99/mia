//! Deterministic, always-on text cleanup — the fidelity-safe first tier of MIA's
//! text intelligence (ADR-008, Phase 1). Pure string→string: it removes filler
//! words, substitutes spoken punctuation ("ponto" → `.`), collapses stutters,
//! normalizes whitespace, and fixes sentence-case — **without a model** and
//! **without ever inventing content**. Every stage is a pure function with
//! `#[cfg(test)]` coverage. See `docs/specs/text-cleanup.md`.

use std::sync::OnceLock;

use regex::Regex;

/// Detected or user-forced language, selecting the rule set. `Other` gets only
/// the language-agnostic core (whitespace + sentence-case).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    PtBr,
    En,
    Other,
}

/// Map an STT language code (e.g. "pt", "en-US") to a rule set.
pub fn lang_from_code(code: &str) -> Lang {
    match code.to_lowercase().as_str() {
        "pt" | "pt-br" | "pt_br" | "por" | "pt-pt" => Lang::PtBr,
        "en" | "en-us" | "en_us" | "en-gb" | "eng" => Lang::En,
        _ => Lang::Other,
    }
}

/// Which cleanup stages are enabled. Mirrors the toggles in the Settings/Hub
/// window (see `docs/specs/settings.md`); defaults match the spec's §4.
#[derive(Clone, Debug)]
pub struct CleanupOptions {
    pub remove_fillers: bool,
    pub spoken_punctuation: bool,
    pub collapse_repeats: bool,
    pub fix_capitalization: bool,
    pub normalize_numbers: bool,
    pub ensure_trailing_period: bool,
    /// User-added stoplist entries (lowercased, whole-token); always removed.
    pub extra_fillers: Vec<String>,
    /// User allow-list; never stripped, overrides the built-in stoplist.
    pub keep_fillers: Vec<String>,
}

impl Default for CleanupOptions {
    fn default() -> Self {
        Self {
            remove_fillers: true,
            spoken_punctuation: true,
            collapse_repeats: true,
            fix_capitalization: true,
            normalize_numbers: true,
            ensure_trailing_period: false,
            extra_fillers: Vec::new(),
            keep_fillers: Vec::new(),
        }
    }
}

/// The single public entry point: raw transcript → polished text. Pure, total,
/// infallible (Rule 9). Empty/whitespace-only input → "". Stages run in the
/// fixed order below; given identical inputs the output is byte-for-byte equal.
pub fn clean(text: &str, lang: Lang, opts: &CleanupOptions) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    let mut s = text.to_string();
    if opts.spoken_punctuation {
        s = substitute_spoken_punctuation(&s, lang);
    }
    if opts.remove_fillers {
        s = remove_fillers(&s, lang, opts);
    }
    if opts.collapse_repeats {
        s = collapse_repeats(&s);
    }
    s = normalize_whitespace(&s); // always on (Rule 4)
    if opts.fix_capitalization {
        s = fix_capitalization(&s, lang);
    }
    if opts.normalize_numbers {
        s = normalize_numbers(&s, lang);
    }
    if opts.ensure_trailing_period {
        s = ensure_trailing_period(&s);
    }
    if s.trim().is_empty() {
        return String::new();
    }
    s
}

// ─────────────────────────────────────────────────────────────────────────────
// Rule 1 — Spoken-punctuation substitution
// ─────────────────────────────────────────────────────────────────────────────

/// How a recognized punctuation word renders, with its spacing semantics.
#[derive(Clone, Copy, Debug)]
enum Punct {
    /// Hugs the previous word; a space follows. `. , ; : ? ! …`
    Trailing(&'static str),
    /// Space before, hugs the next token. `(` `"`(open)
    Open(&'static str),
    /// Hugs the previous token; a space follows. `)` `"`(close)
    Close(&'static str),
    /// Spaces on both sides. `—` `-`
    Spaced(&'static str),
    /// A line/paragraph break; no surrounding spaces. `\n` `\n\n`
    Break(&'static str),
}

enum Piece<'a> {
    Word(&'a str),
    P(Punct),
}

/// Per-language spoken-punctuation map, ordered **longest phrase first** so the
/// first match wins (`ponto e vírgula` before `ponto`, `novo parágrafo` before
/// `nova linha`). Whole-token, case-insensitive matching (Rule 1).
fn punctuation_map(lang: Lang) -> &'static [(&'static str, Punct)] {
    const PT: &[(&str, Punct)] = &[
        ("ponto de interrogação", Punct::Trailing("?")),
        ("ponto de exclamação", Punct::Trailing("!")),
        ("ponto e vírgula", Punct::Trailing(";")),
        ("dois pontos", Punct::Trailing(":")),
        ("novo parágrafo", Punct::Break("\n\n")),
        ("nova linha", Punct::Break("\n")),
        ("abre parênteses", Punct::Open("(")),
        ("fecha parênteses", Punct::Close(")")),
        ("abre aspas", Punct::Open("\"")),
        ("fecha aspas", Punct::Close("\"")),
        ("ponto", Punct::Trailing(".")),
        ("vírgula", Punct::Trailing(",")),
        ("virgula", Punct::Trailing(",")),
        ("interrogação", Punct::Trailing("?")),
        ("exclamação", Punct::Trailing("!")),
        ("reticências", Punct::Trailing("…")),
        ("travessão", Punct::Spaced("—")),
        ("hífen", Punct::Spaced("-")),
    ];
    const EN: &[(&str, Punct)] = &[
        ("exclamation mark", Punct::Trailing("!")),
        ("exclamation point", Punct::Trailing("!")),
        ("question mark", Punct::Trailing("?")),
        ("full stop", Punct::Trailing(".")),
        ("new paragraph", Punct::Break("\n\n")),
        ("new line", Punct::Break("\n")),
        ("open parenthesis", Punct::Open("(")),
        ("close parenthesis", Punct::Close(")")),
        ("open paren", Punct::Open("(")),
        ("close paren", Punct::Close(")")),
        ("open quote", Punct::Open("\"")),
        ("close quote", Punct::Close("\"")),
        ("period", Punct::Trailing(".")),
        ("comma", Punct::Trailing(",")),
        ("semicolon", Punct::Trailing(";")),
        ("colon", Punct::Trailing(":")),
        ("ellipsis", Punct::Trailing("…")),
        ("dash", Punct::Spaced("—")),
        ("hyphen", Punct::Spaced("-")),
    ];
    match lang {
        Lang::PtBr => PT,
        Lang::En => EN,
        Lang::Other => &[],
    }
}

/// True when the `words` phrase matches `tokens` starting at `i`, comparing each
/// token through `norm`. Shared by spoken-punctuation and filler scanning so both
/// stay whole-token and single-level (the caller `continue`s on a hit).
fn phrase_matches_at(tokens: &[&str], i: usize, words: &[&str], norm: impl Fn(&str) -> String) -> bool {
    i + words.len() <= tokens.len() && words.iter().enumerate().all(|(k, w)| norm(tokens[i + k]) == *w)
}

fn substitute_spoken_punctuation(s: &str, lang: Lang) -> String {
    let map = punctuation_map(lang);
    if map.is_empty() {
        return s.to_string();
    }
    let tokens: Vec<&str> = s.split_whitespace().collect();
    let mut pieces: Vec<Piece> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    'outer: while i < tokens.len() {
        for (phrase, punct) in map {
            let words: Vec<&str> = phrase.split(' ').collect();
            if phrase_matches_at(&tokens, i, &words, str::to_lowercase) {
                pieces.push(Piece::P(*punct));
                i += words.len();
                continue 'outer;
            }
        }
        pieces.push(Piece::Word(tokens[i]));
        i += 1;
    }
    join_pieces(&pieces)
}

/// Assemble pieces into a string applying each `Punct`'s spacing semantics.
/// Rough spacing is fine — `normalize_whitespace` (Rule 4) polishes afterwards.
fn join_pieces(pieces: &[Piece]) -> String {
    let mut out = String::new();
    // True when the next word/open should NOT get a leading space.
    let mut attach_next = true;
    for piece in pieces {
        match piece {
            Piece::Word(w) => {
                if !attach_next {
                    out.push(' ');
                }
                out.push_str(w);
                attach_next = false;
            }
            Piece::P(Punct::Trailing(g)) => {
                out.push_str(g);
                attach_next = false;
            }
            Piece::P(Punct::Open(g)) => {
                if !attach_next {
                    out.push(' ');
                }
                out.push_str(g);
                attach_next = true;
            }
            Piece::P(Punct::Close(g)) => {
                out.push_str(g);
                attach_next = false;
            }
            Piece::P(Punct::Spaced(g)) => {
                if !attach_next {
                    out.push(' ');
                }
                out.push_str(g);
                attach_next = false;
            }
            Piece::P(Punct::Break(b)) => {
                out.push_str(b);
                attach_next = true;
            }
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Rule 2 — Filler-word removal (context-guarded)
// ─────────────────────────────────────────────────────────────────────────────

/// Full per-language filler stoplist (single words and phrases). Guarded entries
/// (also content words) are listed in `GUARDED` and only removed when clearly
/// acting as fillers.
fn filler_set(lang: Lang) -> &'static [&'static str] {
    const PT: &[&str] = &[
        "eh", "ah", "hum", "hã", "ahn", "é", "né", "sabe", "então", "tipo",
        "tipo assim", "assim", "meio que",
    ];
    const EN: &[&str] = &[
        "um", "uh", "er", "erm", "hmm", "like", "you know", "i mean",
        "kind of", "sort of", "basically", "actually",
    ];
    match lang {
        Lang::PtBr => PT,
        Lang::En => EN,
        Lang::Other => &[],
    }
}

/// How aggressively a filler may be removed. Content-ambiguous words are only
/// stripped with a clear discourse-marker signal; "então" also strips when it
/// opens a sentence (spec Rule 2). Conservative: when unsure, keep.
#[derive(Clone, Copy, PartialEq)]
enum GuardKind {
    Always,         // unambiguous filler (um, uh, eh, ah …)
    Comma,          // strip only when comma-delimited (é, like, actually …)
    InitialOrComma, // strip when sentence-initial or comma-delimited (então)
}

fn guard_kind(phrase: &str) -> GuardKind {
    match phrase {
        "então" => GuardKind::InitialOrComma,
        "é" | "né" | "sabe" | "tipo" | "tipo assim" | "assim" | "meio que"
        | "like" | "you know" | "i mean" | "kind of" | "sort of" | "basically"
        | "actually" => GuardKind::Comma,
        _ => GuardKind::Always,
    }
}

/// Strip leading/trailing non-alphanumeric (keep apostrophes) and lowercase —
/// for whole-token comparison.
fn norm_word(t: &str) -> String {
    t.trim_matches(|c: char| !(c.is_alphanumeric() || c == '\'')).to_lowercase()
}

/// Build the active filler phrases (built-in stoplist minus `keep`, plus user
/// `extra_fillers`), each as its lowercased words + guard, longest-first.
fn build_filler_phrases(lang: Lang, opts: &CleanupOptions) -> Vec<(Vec<String>, GuardKind)> {
    let keep: Vec<String> = opts.keep_fillers.iter().map(|w| w.to_lowercase()).collect();
    let mut phrases: Vec<(Vec<String>, GuardKind)> = Vec::new();
    for &f in filler_set(lang) {
        if !keep.iter().any(|k| k == f) {
            phrases.push((f.split(' ').map(str::to_string).collect(), guard_kind(f)));
        }
    }
    for f in &opts.extra_fillers {
        let fl = f.to_lowercase();
        if !fl.is_empty() && !keep.contains(&fl) {
            // User-added fillers are explicit → always removed.
            phrases.push((fl.split(' ').map(str::to_string).collect(), GuardKind::Always));
        }
    }
    phrases.sort_by_key(|p| std::cmp::Reverse(p.0.len())); // longest match first
    phrases
}

/// The guard decision: may the phrase occupying `tokens[i..i+n]` be removed given its
/// `kind`? Content-ambiguous fillers need a comma (or, for `InitialOrComma`, a
/// sentence-initial position) — when unsure, keep (spec Rule 2).
fn should_remove(kind: GuardKind, tokens: &[&str], i: usize, n: usize) -> bool {
    let comma_delimited = tokens[i + n - 1].ends_with(',') || (i > 0 && tokens[i - 1].ends_with(','));
    let sentence_initial = i == 0 || tokens[i - 1].ends_with(['.', '?', '!', '…']);
    match kind {
        GuardKind::Always => true,
        GuardKind::Comma => comma_delimited,
        GuardKind::InitialOrComma => sentence_initial || comma_delimited,
    }
}

fn remove_fillers(s: &str, lang: Lang, opts: &CleanupOptions) -> String {
    let phrases = build_filler_phrases(lang, opts);
    if phrases.is_empty() {
        return s.to_string();
    }
    let tokens: Vec<&str> = s.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    'outer: while i < tokens.len() {
        for (words, kind) in &phrases {
            let refs: Vec<&str> = words.iter().map(String::as_str).collect();
            if phrase_matches_at(&tokens, i, &refs, norm_word) && should_remove(*kind, &tokens, i, words.len()) {
                i += words.len(); // drop the whole filler phrase
                continue 'outer;
            }
        }
        out.push(tokens[i]);
        i += 1;
    }
    out.join(" ")
}

// ─────────────────────────────────────────────────────────────────────────────
// Rule 3 — Stutter / repeat collapse
// ─────────────────────────────────────────────────────────────────────────────

/// `th-the` → `the` when the left part is a prefix of the right (a broken-word
/// stutter). Otherwise the token is unchanged.
fn destutter_token(t: &str) -> &str {
    if let Some(idx) = t.find('-') {
        let (l, r) = (&t[..idx], &t[idx + 1..]);
        if !l.is_empty()
            && !r.is_empty()
            && !r.contains('-')
            && l.len() < r.len()
            && l.chars().all(char::is_alphabetic)
            && r.chars().all(char::is_alphabetic)
            && r.to_lowercase().starts_with(&l.to_lowercase())
        {
            return r;
        }
    }
    t
}

fn collapse_repeats(s: &str) -> String {
    let tokens: Vec<&str> = s.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::with_capacity(tokens.len());
    for &raw in &tokens {
        let tok = destutter_token(raw);
        if let Some(&prev) = out.last() {
            let prev_terminates = prev.ends_with(['.', '?', '!', '…']);
            let same = norm_word(prev) == norm_word(tok) && !norm_word(tok).is_empty();
            if same && !prev_terminates {
                continue; // drop adjacent duplicate (keep the first occurrence)
            }
        }
        out.push(tok);
    }
    out.join(" ")
}

// ─────────────────────────────────────────────────────────────────────────────
// Rule 4 — Whitespace normalization (always on)
// ─────────────────────────────────────────────────────────────────────────────

fn cached(slot: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    slot.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

fn normalize_whitespace(s: &str) -> String {
    static SPACE_BEFORE: OnceLock<Regex> = OnceLock::new();
    static AFTER_OPEN: OnceLock<Regex> = OnceLock::new();
    static MULTISPACE: OnceLock<Regex> = OnceLock::new();
    static MULTINL: OnceLock<Regex> = OnceLock::new();

    let s = s.replace('\t', " ");
    // No space before terminators / closing brackets.
    let s = cached(&SPACE_BEFORE, r" +([.,;:?!…)\]])").replace_all(&s, "$1").into_owned();
    // No space right after an opening bracket.
    let s = cached(&AFTER_OPEN, r"([(\[]) +").replace_all(&s, "$1").into_owned();
    // Collapse runs of spaces.
    let s = cached(&MULTISPACE, r"  +").replace_all(&s, " ").into_owned();
    // Trim each line (kills space-before-newline and leading line space).
    let s = s.split('\n').map(str::trim).collect::<Vec<_>>().join("\n");
    // Collapse 3+ newlines to a paragraph break.
    let s = cached(&MULTINL, r"\n{3,}").replace_all(&s, "\n\n").into_owned();
    s.trim().to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// Rules 5-6 — Capitalization
// ─────────────────────────────────────────────────────────────────────────────

fn fix_capitalization(s: &str, lang: Lang) -> String {
    let mut out = String::with_capacity(s.len());
    let mut cap_next = true; // capitalize the next alphabetic char
    for c in s.chars() {
        if cap_next && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            cap_next = false;
        } else {
            out.push(c);
            if matches!(c, '.' | '?' | '!' | '…' | '\n') {
                cap_next = true;
            }
            // Spaces, digits, and openers like '(' leave cap_next as-is, so the
            // next *letter* still gets capitalized (Rule 5: skip leading non-letters).
        }
    }
    if lang == Lang::En {
        out = capitalize_pronoun_i(&out);
    }
    out
}

/// Standalone English `i` (incl. contractions `i'm`, `i'll`) → `I`. En only.
fn capitalize_pronoun_i(s: &str) -> String {
    static I_WORD: OnceLock<Regex> = OnceLock::new();
    cached(&I_WORD, r"\bi\b").replace_all(s, "I").into_owned()
}

// ─────────────────────────────────────────────────────────────────────────────
// Rule 7 — Light number normalization (non-rewriting)
// ─────────────────────────────────────────────────────────────────────────────

fn normalize_numbers(s: &str, lang: Lang) -> String {
    static PT_DEC: OnceLock<Regex> = OnceLock::new();
    static EN_DEC: OnceLock<Regex> = OnceLock::new();
    match lang {
        // pt decimal comma: "3 , 14" → "3,14"
        Lang::PtBr => cached(&PT_DEC, r"(\d) *, *(\d)").replace_all(s, "$1,$2").into_owned(),
        // en decimal point: "3 . 14" → "3.14"
        Lang::En => cached(&EN_DEC, r"(\d) *\. *(\d)").replace_all(s, "$1.$2").into_owned(),
        Lang::Other => s.to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Rule 8 — Trailing period
// ─────────────────────────────────────────────────────────────────────────────

fn ensure_trailing_period(s: &str) -> String {
    let t = s.trim_end();
    if t.is_empty() {
        return s.to_string();
    }
    let last = t.chars().last().unwrap();
    if matches!(last, '.' | '?' | '!' | '…' | ')' | ']' | '}' | '"' | '\'') {
        return s.to_string();
    }
    format!("{t}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> CleanupOptions {
        CleanupOptions::default()
    }

    // ── Rule 1: spoken punctuation ──────────────────────────────────────────
    #[test]
    fn pt_basic_punctuation() {
        assert_eq!(substitute_spoken_punctuation("olá mundo ponto", Lang::PtBr), "olá mundo.");
        assert_eq!(substitute_spoken_punctuation("um vírgula dois", Lang::PtBr), "um, dois");
    }

    #[test]
    fn pt_longest_phrase_wins() {
        // "ponto e vírgula" must beat "ponto"
        assert_eq!(substitute_spoken_punctuation("a ponto e vírgula b", Lang::PtBr), "a; b");
        // "novo parágrafo" must beat "nova linha"
        assert_eq!(
            substitute_spoken_punctuation("a novo parágrafo b", Lang::PtBr),
            "a\n\nb"
        );
        assert_eq!(substitute_spoken_punctuation("a nova linha b", Lang::PtBr), "a\nb");
    }

    #[test]
    fn pt_parentheses_spacing() {
        assert_eq!(
            substitute_spoken_punctuation("diga abre parênteses isso fecha parênteses", Lang::PtBr),
            "diga (isso)"
        );
    }

    #[test]
    fn en_basic_punctuation() {
        assert_eq!(substitute_spoken_punctuation("hello world period", Lang::En), "hello world.");
        assert_eq!(substitute_spoken_punctuation("yes question mark", Lang::En), "yes?");
        assert_eq!(substitute_spoken_punctuation("one comma two", Lang::En), "one, two");
    }

    #[test]
    fn whole_token_only_never_substring() {
        // "comma" must not fire inside "command"
        assert_eq!(substitute_spoken_punctuation("run the command now", Lang::En), "run the command now");
        // "ponto" must not fire inside "apontamento"
        assert_eq!(substitute_spoken_punctuation("fiz um apontamento", Lang::PtBr), "fiz um apontamento");
    }

    #[test]
    fn other_lang_leaves_punctuation_words() {
        assert_eq!(substitute_spoken_punctuation("hola mundo period", Lang::Other), "hola mundo period");
    }

    // ── Rule 2: filler removal ──────────────────────────────────────────────
    #[test]
    fn en_unambiguous_fillers_removed() {
        let r = remove_fillers("um so uh the thing", Lang::En, &opts());
        assert_eq!(r, "so the thing");
    }

    #[test]
    fn en_discourse_like_removed_when_comma_delimited() {
        let r = remove_fillers("it's, like, broken", Lang::En, &opts());
        assert_eq!(r, "it's, broken");
    }

    #[test]
    fn en_verb_like_is_kept() {
        assert_eq!(remove_fillers("I like coffee", Lang::En, &opts()), "I like coffee");
        assert_eq!(remove_fillers("it works like this", Lang::En, &opts()), "it works like this");
    }

    #[test]
    fn pt_verb_e_is_kept_filler_e_removed() {
        assert_eq!(remove_fillers("ela é médica", Lang::PtBr, &opts()), "ela é médica");
        assert_eq!(remove_fillers("é, eu acho", Lang::PtBr, &opts()), "eu acho");
    }

    #[test]
    fn multiword_filler_you_know() {
        assert_eq!(remove_fillers("well, you know, it works", Lang::En, &opts()), "well, it works");
        // not a filler when not comma-delimited
        assert_eq!(remove_fillers("do you know him", Lang::En, &opts()), "do you know him");
    }

    #[test]
    fn extra_and_keep_fillers() {
        let mut o = opts();
        o.extra_fillers = vec!["foo".into()];
        assert_eq!(remove_fillers("foo bar foo baz", Lang::En, &o), "bar baz");

        let mut o2 = opts();
        o2.keep_fillers = vec!["like".into()];
        assert_eq!(remove_fillers("it's, like, broken", Lang::En, &o2), "it's, like, broken");
    }

    #[test]
    fn phrase_matches_at_is_whole_token_and_bounds_checked() {
        let tokens = vec!["you", "know", "it"];
        assert!(phrase_matches_at(&tokens, 0, &["you", "know"], str::to_lowercase));
        // Past the end: no panic, no match.
        assert!(!phrase_matches_at(&tokens, 2, &["it", "is"], str::to_lowercase));
        // norm_word strips trailing punctuation before comparing.
        let punct = vec!["like,", "broken"];
        assert!(phrase_matches_at(&punct, 0, &["like"], norm_word));
        assert!(!phrase_matches_at(&punct, 0, &["like"], str::to_lowercase)); // "like," != "like"
    }

    #[test]
    fn should_remove_honors_guard_kind() {
        // Always: removed regardless of context.
        assert!(should_remove(GuardKind::Always, &["um", "ok"], 0, 1));
        // Comma: only when comma-delimited.
        assert!(!should_remove(GuardKind::Comma, &["like", "this"], 0, 1));
        assert!(should_remove(GuardKind::Comma, &["a", "like,", "b"], 1, 1));
        assert!(should_remove(GuardKind::Comma, &["a,", "like", "b"], 1, 1)); // preceding comma
        // InitialOrComma: sentence-initial counts even without a comma.
        assert!(should_remove(GuardKind::InitialOrComma, &["então", "foi"], 0, 1));
        assert!(!should_remove(GuardKind::InitialOrComma, &["e", "então", "foi"], 1, 1));
        assert!(should_remove(GuardKind::InitialOrComma, &["fim.", "então", "foi"], 1, 1)); // after a period
    }

    // ── Rule 3: repeat collapse ─────────────────────────────────────────────
    #[test]
    fn adjacent_duplicates_collapse() {
        assert_eq!(collapse_repeats("the the cat"), "the cat");
        assert_eq!(collapse_repeats("eu eu fui"), "eu fui");
    }

    #[test]
    fn broken_word_stutter_collapses() {
        assert_eq!(collapse_repeats("th-the cat"), "the cat");
        assert_eq!(collapse_repeats("wh-what is it"), "what is it");
    }

    #[test]
    fn non_adjacent_and_cross_sentence_preserved() {
        assert_eq!(collapse_repeats("good. good morning"), "good. good morning");
        assert_eq!(collapse_repeats("very good very good"), "very good very good");
    }

    // ── Rule 4: whitespace ──────────────────────────────────────────────────
    #[test]
    fn whitespace_normalization() {
        assert_eq!(normalize_whitespace("a   b"), "a b");
        assert_eq!(normalize_whitespace("a ."), "a.");
        assert_eq!(normalize_whitespace("( x )"), "(x)");
        assert_eq!(normalize_whitespace("trailing   "), "trailing");
        assert_eq!(normalize_whitespace("a\n\n\n\nb"), "a\n\nb");
    }

    // ── Rules 5-6: capitalization ───────────────────────────────────────────
    #[test]
    fn sentence_capitalization() {
        assert_eq!(fix_capitalization("hello. world", Lang::En), "Hello. World");
        assert_eq!(fix_capitalization("olá. mundo", Lang::PtBr), "Olá. Mundo");
    }

    #[test]
    fn capitalization_preserves_interior_and_skips_leading_nonletter() {
        assert_eq!(fix_capitalization("buy iPhone now", Lang::En), "Buy iPhone now");
        assert_eq!(fix_capitalization("(hello)", Lang::En), "(Hello)");
    }

    #[test]
    fn en_pronoun_i() {
        assert_eq!(fix_capitalization("then i think i'm late", Lang::En), "Then I think I'm late");
    }

    #[test]
    fn pronoun_i_only_for_english() {
        // pt/Other must not touch a lone "i"
        assert_eq!(fix_capitalization("vou ali i depois", Lang::PtBr), "Vou ali i depois");
    }

    // ── Rule 7: numbers ─────────────────────────────────────────────────────
    #[test]
    fn light_number_spacing() {
        assert_eq!(normalize_numbers("3 , 14", Lang::PtBr), "3,14");
        assert_eq!(normalize_numbers("3 . 14", Lang::En), "3.14");
        // never converts number words
        assert_eq!(normalize_numbers("twenty three", Lang::En), "twenty three");
    }

    // ── Rule 8: trailing period ─────────────────────────────────────────────
    #[test]
    fn trailing_period() {
        assert_eq!(ensure_trailing_period("hello"), "hello.");
        assert_eq!(ensure_trailing_period("hello."), "hello.");
        assert_eq!(ensure_trailing_period("really?"), "really?");
        assert_eq!(ensure_trailing_period(""), "");
    }

    // ── Rule 9 + clean() end-to-end ─────────────────────────────────────────
    #[test]
    fn clean_empty_and_all_filler() {
        assert_eq!(clean("   ", Lang::En, &opts()), "");
        assert_eq!(clean("um uh er", Lang::En, &opts()), "");
    }

    #[test]
    fn clean_is_deterministic() {
        let input = "um so, like, the the thing is broken period";
        let a = clean(input, Lang::En, &opts());
        let b = clean(input, Lang::En, &opts());
        assert_eq!(a, b);
    }

    #[test]
    fn clean_end_to_end_en() {
        let out = clean("um so the the report is done period", Lang::En, &opts());
        assert_eq!(out, "So the report is done.");
    }

    #[test]
    fn clean_end_to_end_pt() {
        let out = clean("então eu fiz o relatório ponto", Lang::PtBr, &opts());
        assert_eq!(out, "Eu fiz o relatório.");
    }

    #[test]
    fn clean_is_idempotent_on_clean_text() {
        let once = clean("the report is done.", Lang::En, &opts());
        let twice = clean(&once, Lang::En, &opts());
        assert_eq!(once, twice);
    }

    #[test]
    fn clean_all_toggles_off_keeps_text() {
        let o = CleanupOptions {
            remove_fillers: false,
            spoken_punctuation: false,
            collapse_repeats: false,
            fix_capitalization: false,
            normalize_numbers: false,
            ensure_trailing_period: false,
            extra_fillers: vec![],
            keep_fillers: vec![],
        };
        // Only whitespace normalization (Rule 4, always on) applies.
        assert_eq!(clean("um  the   thing", Lang::En, &o), "um the thing");
    }

    // ── Data tables + glue ──────────────────────────────────────────────────
    #[test]
    fn data_tables_present() {
        assert!(!filler_set(Lang::PtBr).is_empty());
        assert!(!filler_set(Lang::En).is_empty());
        assert!(filler_set(Lang::Other).is_empty());
        assert!(!punctuation_map(Lang::PtBr).is_empty());
        assert!(!punctuation_map(Lang::En).is_empty());
        assert!(punctuation_map(Lang::Other).is_empty());
    }

    #[test]
    fn lang_from_code_maps() {
        assert_eq!(lang_from_code("pt"), Lang::PtBr);
        assert_eq!(lang_from_code("pt-BR"), Lang::PtBr);
        assert_eq!(lang_from_code("en"), Lang::En);
        assert_eq!(lang_from_code("EN-US"), Lang::En);
        assert_eq!(lang_from_code("fr"), Lang::Other);
    }
}
