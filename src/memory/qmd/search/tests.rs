//! Unit tests for search sub-modules.

#[cfg(test)]
mod tests {
    use crate::memory::qmd_memory::search::extract_answer;
    use crate::memory::qmd_memory::utils::{
        extract_speaker_from_query, extract_speakers, is_likely_speaker, resolve_pronouns,
    };

    #[test]
    fn test_extract_speakers() {
        let text = "Caroline: Hello\n[James]: Hi\nSpeaker: Alice\nPerson: Robert\nGuest: Emma";
        let speakers = extract_speakers(text);
        assert!(speakers.contains(&"Caroline".to_string()));
        assert!(speakers.contains(&"James".to_string()));
        assert!(speakers.contains(&"Alice".to_string()));
        assert!(speakers.contains(&"Robert".to_string()));
        assert!(speakers.contains(&"Emma".to_string()));
    }

    #[test]
    fn test_extract_speaker_from_query() {
        assert_eq!(
            extract_speaker_from_query("Who is Caroline?"),
            Some("Caroline".to_string())
        );
        assert_eq!(
            extract_speaker_from_query("What did James say?"),
            Some("James".to_string())
        );
        assert_eq!(
            extract_speaker_from_query("When was Alice there?"),
            Some("Alice".to_string())
        );
        assert_eq!(
            extract_speaker_from_query("Where is Robert?"),
            Some("Robert".to_string())
        );
        assert_eq!(
            extract_speaker_from_query("Why did Emma laugh?"),
            Some("Emma".to_string())
        );
    }

    #[test]
    fn test_resolve_pronouns() {
        let speakers = vec!["Caroline".to_string(), "James".to_string()];

        // Single female candidate
        assert_eq!(
            resolve_pronouns("What did she say?", &speakers),
            "What did Caroline say?"
        );

        // Single male candidate
        assert_eq!(
            resolve_pronouns("What did he say?", &speakers),
            "What did James say?"
        );

        // Multiple female candidates - no resolution
        let speakers_multiple = vec!["Caroline".to_string(), "Alice".to_string()];
        assert_eq!(
            resolve_pronouns("What did she say?", &speakers_multiple),
            "What did she say?"
        );
    }

    #[test]
    fn test_is_likely_speaker() {
        assert!(is_likely_speaker("Caroline"));
        assert!(is_likely_speaker("James"));
        assert!(!is_likely_speaker("Who"));
        assert!(!is_likely_speaker("What"));
        assert!(!is_likely_speaker("She"));
        assert!(!is_likely_speaker("The"));
    }

    #[test]
    fn test_extract_answer_date() {
        let result = extract_answer("The event took place on 15 January 2023 in New York.", "2");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "15 January 2023");
    }

    #[test]
    fn test_extract_answer_opinion() {
        let result = extract_answer(
            "I think it's a wonderful idea to travel together. We should plan this.",
            "3",
        );
        assert!(result.is_some());
        assert!(result.unwrap().to_lowercase().contains("think"));
    }

    #[test]
    fn test_extract_answer_future() {
        let result = extract_answer(
            "I have decided to start a new business next month. The planning is complete.",
            "4",
        );
        assert!(result.is_some());
        assert!(result.unwrap().to_lowercase().contains("decided"));
    }
}
