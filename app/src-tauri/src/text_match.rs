//! Shared whole-word token machinery for the text stage. Both the custom dictionary
//! (`dictionary.rs`) and snippets (`snippets.rs`) match on whole words and rebuild the
//! string preserving the original punctuation, so the *mechanics* — split into
//! alternating word/separator runs, then replay a span→replacement plan over them —
//! live here once. Each caller keeps its own matching rules (fuzzy/case for the
//! dictionary, accent-fold/anchor for snippets) and only shares this scaffolding.

use std::collections::HashMap;

/// One run of the input: an alphanumeric `Word` or a `Sep` (everything else). Splitting
/// this way makes matching whole-word and lets reconstruction re-emit the exact
/// original separators between untouched words.
pub enum Tok {
    Word(String),
    Sep(String),
}

/// Split `s` into alternating word (Unicode alphanumeric) and separator runs.
pub fn tokenize(s: &str) -> Vec<Tok> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    let mut cur_word = false;
    for ch in s.chars() {
        let is_word = ch.is_alphanumeric();
        if cur.is_empty() {
            cur_word = is_word;
        } else if is_word != cur_word {
            toks.push(flush(&cur, cur_word));
            cur.clear();
            cur_word = is_word;
        }
        cur.push(ch);
    }
    if !cur.is_empty() {
        toks.push(flush(&cur, cur_word));
    }
    toks
}

fn flush(run: &str, is_word: bool) -> Tok {
    if is_word {
        Tok::Word(run.to_string())
    } else {
        Tok::Sep(run.to_string())
    }
}

/// A replacement plan keyed by word index: `word_pos -> (words_consumed, replacement)`.
pub type Plan = HashMap<usize, (usize, String)>;

/// Replay `plan` over `toks`, dropping the interior separators of a multi-word match.
/// `plan` is keyed by word position (separators don't count); a placed match consumes
/// its whole span. Untouched words/separators are re-emitted verbatim.
pub fn reconstruct(toks: &[Tok], plan: &Plan) -> String {
    let mut out = String::new();
    let mut wpos = 0usize;
    let mut i = 0usize;
    while i < toks.len() {
        match &toks[i] {
            Tok::Sep(s) => {
                out.push_str(s);
                i += 1;
            }
            Tok::Word(w) => {
                let Some((k, repl)) = plan.get(&wpos) else {
                    out.push_str(w);
                    i += 1;
                    wpos += 1;
                    continue;
                };
                out.push_str(repl);
                i = consume_words(toks, i, *k);
                wpos += k;
            }
        }
    }
    out
}

/// Advance past `k` words (and the separators between them) starting at token `i`.
fn consume_words(toks: &[Tok], mut i: usize, k: usize) -> usize {
    let mut consumed = 0;
    while i < toks.len() && consumed < k {
        if matches!(toks[i], Tok::Word(_)) {
            consumed += 1;
        }
        i += 1;
        if consumed == k {
            break;
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(toks: &[Tok]) -> Vec<&str> {
        toks.iter()
            .filter_map(|t| match t {
                Tok::Word(w) => Some(w.as_str()),
                Tok::Sep(_) => None,
            })
            .collect()
    }

    #[test]
    fn tokenize_alternates_words_and_separators() {
        let toks = tokenize("open mia, now");
        assert_eq!(words(&toks), vec!["open", "mia", "now"]);
        // The interior separators (" ", ", ") survive for reconstruction.
        let rebuilt = reconstruct(&toks, &Plan::new());
        assert_eq!(rebuilt, "open mia, now");
    }

    #[test]
    fn tokenize_empty_is_empty() {
        assert!(tokenize("").is_empty());
    }

    #[test]
    fn reconstruct_replaces_single_word() {
        let toks = tokenize("open mia now");
        let mut plan = Plan::new();
        plan.insert(1, (1, "MIA".to_string())); // word index 1 = "mia"
        assert_eq!(reconstruct(&toks, &plan), "open MIA now");
    }

    #[test]
    fn reconstruct_collapses_multiword_match_dropping_interior_seps() {
        let toks = tokenize("i use react js daily");
        let mut plan = Plan::new();
        plan.insert(2, (2, "React".to_string())); // words "react js" → "React"
        assert_eq!(reconstruct(&toks, &plan), "i use React daily");
    }

    #[test]
    fn reconstruct_preserves_punctuation_around_match() {
        let toks = tokenize("(react js) here");
        let mut plan = Plan::new();
        plan.insert(0, (2, "React".to_string()));
        assert_eq!(reconstruct(&toks, &plan), "(React) here");
    }

    #[test]
    fn reconstruct_empty_plan_is_identity() {
        let toks = tokenize("hello,  world!");
        assert_eq!(reconstruct(&toks, &Plan::new()), "hello,  world!");
    }
}
