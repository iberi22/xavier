use std::{fs, path::Path};

fn scanner_dockerfile() -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".oss-scanner/Dockerfile"))
        .expect("OSS scanner Dockerfile must exist")
}

#[test]
fn oss_scanner_dockerfile_contract() {
    let dockerfile = scanner_dockerfile();
    assert!(
        dockerfile
            .lines()
            .any(|line| line.split_whitespace().eq(["COPY", ".", "/src"])),
        "Scanner checkout must be copied with COPY . /src"
    );
    assert!(
        dockerfile.lines().any(|line| line
            .split_whitespace()
            .eq(["RUN", "cargo", "fetch", "--locked"])),
        "Scanner dependencies must be fetched with cargo fetch --locked"
    );
}

#[test]
fn oss_scanner_toolchain_matches_ci_msrv() {
    let dockerfile = scanner_dockerfile();
    let image = dockerfile
        .lines()
        .find_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some("FROM"))
                .then(|| words.next())
                .flatten()
        })
        .expect("Scanner Dockerfile must declare a FROM image");
    let version = image
        .strip_prefix("rust:")
        .expect("Scanner must use an official Rust image")
        .split('-')
        .next()
        .expect("Rust image must specify a toolchain");
    let ci =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/ci.yml"))
            .expect("CI workflow must exist");
    let workflow: serde_yaml::Value = serde_yaml::from_str(&ci).expect("CI workflow must be YAML");
    let msrv = workflow["jobs"]["rust-msrv"]["steps"]
        .as_sequence()
        .expect("CI must have a rust-msrv job with steps")
        .iter()
        .filter(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("dtolnay/rust-toolchain@"))
        })
        .find_map(|step| step["with"]["toolchain"].as_str())
        .expect("CI MSRV job must pin its Rust toolchain");
    assert_eq!(version, msrv, "Scanner Rust toolchain must match CI MSRV");
}

#[test]
fn oss_scanner_threat_model_exists() {
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".oss-scanner/threat_model.md")
            .is_file(),
        "Scanner threat model must exist at .oss-scanner/threat_model.md"
    );
}
