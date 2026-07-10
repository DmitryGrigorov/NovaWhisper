//! Personal dictionary terms and voice-activated snippets.
//!
//! Dictionary terms are sent to the gateway with each utterance (STT keyword
//! boosting + polish bias). Snippets are expanded client-side on the final
//! transcript so they work even with polish disabled.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Snippet {
    /// Spoken trigger phrase, e.g. "my email signature".
    pub trigger: String,
    /// Text block inserted in its place.
    pub expansion: String,
}

pub struct SnippetEngine {
    snippets: Vec<Snippet>,
}

/// Lowercase, strip punctuation, collapse whitespace.
fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_ascii_punctuation())
        .collect::<String>()
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

impl SnippetEngine {
    pub fn new(snippets: Vec<Snippet>) -> Self {
        Self { snippets }
    }

    /// Expand snippet triggers in `text`.
    ///
    /// - If the whole utterance is a trigger, the expansion replaces it.
    /// - Multi-word triggers found inline (word-boundary match, ignoring case
    ///   and punctuation) are replaced in place. Single-word triggers only
    ///   match as the whole utterance, to avoid false positives mid-sentence.
    pub fn expand(&self, text: &str) -> String {
        let norm_text = normalize(text);
        for s in &self.snippets {
            if normalize(&s.trigger) == norm_text {
                return s.expansion.clone();
            }
        }

        let mut result = text.to_string();
        for s in &self.snippets {
            let trigger_words: Vec<String> =
                normalize(&s.trigger).split(' ').map(String::from).collect();
            if trigger_words.len() < 2 {
                continue;
            }
            result = replace_inline(&result, &trigger_words, &s.expansion);
        }
        result
    }
}

/// Replace word-boundary occurrences of `trigger_words` inside `text`,
/// comparing words case- and punctuation-insensitively.
fn replace_inline(text: &str, trigger_words: &[String], expansion: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let matches = i + trigger_words.len() <= words.len()
            && trigger_words
                .iter()
                .enumerate()
                .all(|(j, t)| normalize(words[i + j]) == *t);
        if matches {
            out.push(expansion.to_string());
            i += trigger_words.len();
        } else {
            out.push(words[i].to_string());
            i += 1;
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> SnippetEngine {
        SnippetEngine::new(vec![
            Snippet {
                trigger: "my signature".into(),
                expansion: "Best regards,\nDimitar Grigorov".into(),
            },
            Snippet {
                trigger: "standup link".into(),
                expansion: "https://meet.example.com/standup".into(),
            },
            Snippet {
                trigger: "brb".into(),
                expansion: "Be right back!".into(),
            },
        ])
    }

    #[test]
    fn whole_utterance_trigger_expands() {
        assert_eq!(engine().expand("My signature."), "Best regards,\nDimitar Grigorov");
    }

    #[test]
    fn inline_multiword_trigger_expands() {
        assert_eq!(
            engine().expand("Join at standup link please"),
            "Join at https://meet.example.com/standup please"
        );
    }

    #[test]
    fn single_word_trigger_only_matches_whole_utterance() {
        assert_eq!(engine().expand("brb"), "Be right back!");
        assert_eq!(engine().expand("I said brb earlier"), "I said brb earlier");
    }

    #[test]
    fn no_trigger_passthrough() {
        assert_eq!(engine().expand("Nothing to see here."), "Nothing to see here.");
    }
}
