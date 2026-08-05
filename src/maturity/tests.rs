#[cfg(test)]
mod tests {
    use crate::maturity::MaturityScanner;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_scanner_auto_generates_when_missing() {
        let temp_dir = tempdir().unwrap();
        let anchor_path = temp_dir.path().join(".xavier").join("maturity-anchors.json");

        assert!(!anchor_path.exists());

        // This should auto-generate the file in the temp directory and succeed.
        let scanner_res = MaturityScanner::new(&anchor_path, ".");
        assert!(scanner_res.is_ok());
        assert!(anchor_path.exists());

        let content = fs::read_to_string(&anchor_path).unwrap();
        assert!(content.contains("memory-rag"));
    }

    #[test]
    fn test_scanner_fails_gracefully_when_non_writable() {
        let temp_dir = tempdir().unwrap();
        // Create a non-writable path by making a directory where the file should be
        let anchor_path = temp_dir.path().join("maturity-anchors.json");
        fs::create_dir(&anchor_path).unwrap();

        let scanner_res = MaturityScanner::new(&anchor_path, ".");
        assert!(scanner_res.is_err());
        let err_msg = scanner_res.err().unwrap().to_string();
        assert!(err_msg.contains("Maturity scanner anchors manifest not found"));
        assert!(err_msg.contains("Action Required:"));
    }
}
