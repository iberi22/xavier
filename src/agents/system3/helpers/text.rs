//! Text processing helpers for System3
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::agents::system1::RetrievedDocument;
use crate::utils::crypto::sha256_hex;

/// Snippet.
pub(crate) fn snippet(text: &str, max_chars: usize) -> String {
    text.chars()
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Query fingerprint.
pub(crate) fn query_fingerprint(query: &str) -> String {
    sha256_hex(query.as_bytes())[..12].to_string()
}

/// Top non empty contents.
pub(crate) fn top_non_empty_contents(docs: &[RetrievedDocument], limit: usize) -> Vec<String> {
    docs.iter()
        .filter_map(|doc| {
            let text = doc.content.trim();
            (!text.is_empty()).then(|| text.to_string())
        })
        .take(limit)
        .collect()
}

/// Split sentences.
pub(crate) fn split_sentences(text: &str) -> Vec<String> {
    text.split(['.', '!', '?'])
        .map(str::trim)
        .filter(|sentence| !sentence.is_empty())
        .map(|sentence| sentence.to_string())
        .collect()
}

/// Trim speaker prefix.
pub(crate) fn trim_speaker_prefix(text: &str) -> &str {
    text.split_once(':')
        .map(|(_, rest)| rest.trim())
        .filter(|rest| !rest.is_empty())
        .unwrap_or(text.trim())
}

/// Extract prefixed speaker.
pub(crate) fn extract_prefixed_speaker(text: &str) -> Option<String> {
    let (speaker, _) = text.split_once(':')?;
    let speaker = speaker.trim();
    if speaker.is_empty() || speaker.split_whitespace().count() > 3 {
        return None;
    }
    speaker
        .chars()
        .next()
        .filter(|ch| ch.is_ascii_uppercase())
        .map(|_| speaker.to_string())
}

/// Is low signal conversation sentence.
pub(crate) fn is_low_signal_conversation_sentence(sentence: &str) -> bool {
    let trimmed = trim_speaker_prefix(sentence).trim();
    if trimmed.is_empty() {
        return true;
    }

    let lowered = trimmed.to_lowercase();
    let compact = lowered
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch.is_whitespace() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>();
    let tokens: Vec<&str> = compact.split_whitespace().collect();

    if tokens.len() <= 4
        && tokens.iter().all(|token| {
            matches!(
                *token,
                "hey"
                    | "hi"
                    | "hello"
                    | "thanks"
                    | "thank"
                    | "sorry"
                    | "wow"
                    | "cool"
                    | "great"
                    | "nice"
                    | "yeah"
                    | "yep"
                    | "ok"
                    | "okay"
            )
        })
    {
        return true;
    }

    let filler_phrases = [
        "good to see you",
        "long time no see",
        "thanks for asking",
        "sorry to hear that",
        "sorry about your job",
        "thanks",
        "hey ",
        "hi ",
    ];

    filler_phrases
        .iter()
        .any(|phrase| lowered == *phrase || lowered.starts_with(phrase))
}

/// Split meaningful sentences.
pub(crate) fn split_meaningful_sentences(text: &str) -> Vec<String> {
    split_sentences(text)
        .into_iter()
        .filter(|sentence| !is_low_signal_conversation_sentence(sentence))
        .collect()
}

/// Query lower.
pub(crate) fn query_lower(query: &str) -> String {
    query.to_lowercase()
}

/// Query terms.
pub(crate) fn query_terms(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|term| {
            let term = *term;
            term.len() > 2
                && !matches!(
                    term,
                    "when"
                        | "what"
                        | "have"
                        | "that"
                        | "with"
                        | "from"
                        | "into"
                        | "this"
                        | "your"
                        | "about"
                        | "did"
                        | "does"
                        | "the"
                        | "and"
                        | "for"
                        | "who"
                        | "why"
                        | "how"
                        | "where"
                        | "was"
                        | "were"
                        | "after"
                        | "before"
                        | "they"
                        | "them"
                        | "went"
                )
        })
        .map(|term| term.to_string())
        .collect()
}

/// Query phrases.
pub(crate) fn query_phrases(terms: &[String]) -> Vec<String> {
    if terms.len() < 2 {
        return Vec::new();
    }

    terms.windows(2).map(|window| window.join(" ")).collect()
}
