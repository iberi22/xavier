use std::path::Path;
use tokenizers::Tokenizer;

pub struct TextTokenizer {
    tokenizer: Tokenizer,
}

impl TextTokenizer {
    pub fn new(model_dir: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let tokenizer_path = model_dir.join("tokenizer.json");
        let tokenizer = Tokenizer::from_file(tokenizer_path)?;
        Ok(Self { tokenizer })
    }

    pub fn encode_query(
        &self,
        text: &str,
    ) -> Result<(Vec<i64>, Vec<i64>), Box<dyn std::error::Error + Send + Sync>> {
        let prompt = format!("task: search result | query: {}", text);
        self.encode(&prompt)
    }

    pub fn encode_document(
        &self,
        title: Option<&str>,
        content: &str,
    ) -> Result<(Vec<i64>, Vec<i64>), Box<dyn std::error::Error + Send + Sync>> {
        let prompt = match title {
            Some(t) => format!("title: {} | text: {}", t, content),
            None => format!("title: none | text: {}", content),
        };
        self.encode(&prompt)
    }

    fn encode(
        &self,
        text: &str,
    ) -> Result<(Vec<i64>, Vec<i64>), Box<dyn std::error::Error + Send + Sync>> {
        let mut tokenizer = self.tokenizer.clone();

        let truncation = tokenizers::utils::truncation::TruncationParams {
            max_length: 8192,
            direction: tokenizers::utils::truncation::TruncationDirection::Right,
            ..Default::default()
        };
        let _ = tokenizer.with_truncation(Some(truncation));

        let padding = tokenizers::utils::padding::PaddingParams {
            strategy: tokenizers::utils::padding::PaddingStrategy::BatchLongest,
            direction: tokenizers::utils::padding::PaddingDirection::Right,
            pad_id: 0,
            pad_type_id: 0,
            pad_token: String::from("<pad>"),
            pad_to_multiple_of: None,
        };
        let _ = tokenizer.with_padding(Some(padding));

        let encoding = tokenizer.encode(text, true)?;

        let input_ids = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let attention_mask = encoding
            .get_attention_mask()
            .iter()
            .map(|&mask| mask as i64)
            .collect();

        Ok((input_ids, attention_mask))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_encode_query() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tokenizer.json");
        let mut file = File::create(&path).unwrap();

        let dummy_json = r#"{
            "version": "1.0",
            "truncation": null,
            "padding": null,
            "added_tokens": [{"id": 0, "content": "<pad>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true}],
            "normalizer": null,
            "pre_tokenizer": null,
            "post_processor": null,
            "decoder": null,
            "model": {
                "type": "BPE",
                "dropout": null,
                "unk_token": null,
                "continuing_subword_prefix": null,
                "end_of_word_suffix": null,
                "fuse_unk": false,
                "byte_fallback": false,
                "vocab": {"<pad>": 0, "task:": 1, "search": 2, "result": 3, "|": 4, "query:": 5, "hello": 6},
                "merges": []
            }
        }"#;
        file.write_all(dummy_json.as_bytes()).unwrap();

        let tokenizer = TextTokenizer::new(dir.path()).unwrap();
        let (ids, mask) = tokenizer.encode_query("hello").unwrap();

        assert_eq!(ids.len(), mask.len());
        assert!(!ids.is_empty());
    }

    #[test]
    fn test_encode_document() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tokenizer.json");
        let mut file = File::create(&path).unwrap();

        let dummy_json = r#"{
            "version": "1.0",
            "truncation": null,
            "padding": null,
            "added_tokens": [{"id": 0, "content": "<pad>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true}],
            "normalizer": null,
            "pre_tokenizer": null,
            "post_processor": null,
            "decoder": null,
            "model": {
                "type": "BPE",
                "dropout": null,
                "unk_token": null,
                "continuing_subword_prefix": null,
                "end_of_word_suffix": null,
                "fuse_unk": false,
                "byte_fallback": false,
                "vocab": {"<pad>": 0, "title:": 1, "my": 2, "title": 3, "|": 4, "text:": 5, "hello": 6, "none": 7},
                "merges": []
            }
        }"#;
        file.write_all(dummy_json.as_bytes()).unwrap();

        let tokenizer = TextTokenizer::new(dir.path()).unwrap();
        let (ids_with_title, mask_with_title) = tokenizer
            .encode_document(Some("my title"), "hello")
            .unwrap();
        assert_eq!(ids_with_title.len(), mask_with_title.len());
        assert!(!ids_with_title.is_empty());

        let (ids_no_title, mask_no_title) = tokenizer.encode_document(None, "hello").unwrap();
        assert_eq!(ids_no_title.len(), mask_no_title.len());
        assert!(!ids_no_title.is_empty());
    }
}
