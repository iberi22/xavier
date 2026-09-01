use std::fs;
use std::path::Path;

#[test]
fn test_adr_030_structure_and_line_count() {
    let adr_path = Path::new("docs/adr/ADR-030-panel-browser-compat.md");
    assert!(adr_path.exists(), "ADR-030 file should exist");

    let content = fs::read_to_string(adr_path).expect("failed to read ADR-030");
    let line_count = content.lines().count();
    assert!(
        line_count >= 80,
        "ADR-030 line count should be at least 80, got {line_count}"
    );

    assert!(
        content.contains("## Context"),
        "ADR-030 missing ## Context section"
    );
    assert!(
        content.contains("## Decision"),
        "ADR-030 missing ## Decision section"
    );
    assert!(
        content.contains("## Consequences"),
        "ADR-030 missing ## Consequences section"
    );
    assert!(
        content.contains("## Alternatives"),
        "ADR-030 missing ## Alternatives section"
    );

    assert!(
        content.contains("__TAURI_INTERNALS__"),
        "ADR-030 should mention __TAURI_INTERNALS__"
    );
}

#[test]
fn test_adr_031_structure_and_line_count() {
    let adr_path = Path::new("docs/adr/ADR-031-swal-versioning-gate.md");
    assert!(adr_path.exists(), "ADR-031 file should exist");

    let content = fs::read_to_string(adr_path).expect("failed to read ADR-031");
    let line_count = content.lines().count();
    assert!(
        line_count >= 60,
        "ADR-031 line count should be at least 60, got {line_count}"
    );

    assert!(
        content.contains("## Context"),
        "ADR-031 missing ## Context section"
    );
    assert!(
        content.contains("## Decision"),
        "ADR-031 missing ## Decision section"
    );
    assert!(
        content.contains("## Consequences"),
        "ADR-031 missing ## Consequences section"
    );
    assert!(
        content.contains("## Alternatives"),
        "ADR-031 missing ## Alternatives section"
    );

    assert!(
        content.contains("VERSIONING.md"),
        "ADR-031 should mention VERSIONING.md"
    );
}

#[test]
fn test_srs_req_044_present_and_verifiable() {
    let srs_path = Path::new("docs/SRS/REQUIREMENTS.md");
    assert!(srs_path.exists(), "SRS REQUIREMENTS file should exist");

    let content = fs::read_to_string(srs_path).expect("failed to read SRS REQUIREMENTS");
    assert!(
        content.contains("REQ-044"),
        "SRS should contain REQ-044 requirement"
    );

    let req_044_section = content
        .split("REQ-044")
        .nth(1)
        .expect("REQ-044 section content missing");

    assert!(
        req_044_section.contains("browser") || req_044_section.contains("Panel"),
        "REQ-044 section should describe panel browser compatibility"
    );
}
