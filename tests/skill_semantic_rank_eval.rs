//! Offline eval harness for skill semantic ranking (issue #302).
//!
//! Labeled `query -> expected skill` set evaluated with a deterministic mocked
//! embedder. Zero network: the mock is a pure function (no sockets, no backend).

use xavier::context::{skill_embed_text, SkillRegistry};

/// Topic keyword lists, synonyms included so queries paraphrase descriptions.
const TOPICS: &[&[&str]] = &[
    &[
        "pull request",
        "pull-request",
        "github",
        "diff",
        "merge",
        "merging",
        "review",
        "verify",
        "evidence",
        "checklist",
    ],
    &[
        "rust",
        "cargo",
        "unit test",
        "test suite",
        "test failure",
        "broke",
    ],
    &[
        "deploy",
        "release",
        "publish",
        "production",
        "hosting",
        "rollout",
        "ship",
    ],
    &[
        "memory", "memories", "recall", "remember", "archive", "notes", "decided",
    ],
    &[
        "network",
        "timeout",
        "timing out",
        "time out",
        "slow",
        "latency",
        "connection",
        "offline",
        "load",
    ],
    &[
        "auth",
        "token",
        "secret",
        "login",
        "permission",
        "secure",
        "api key",
    ],
    &[
        "database", "postgres", "schema", "migrate", "sql", "table", "backfill",
    ],
    &[
        "chat",
        "meeting",
        "summarize",
        "summary",
        "decision",
        "action item",
        "minutes",
    ],
];

/// Deterministic mocked embedding: L2-normalized topic hit counts.
fn mock_embed(text: &str) -> Vec<f32> {
    let lower = text.to_lowercase();
    let mut vector = vec![0.0_f32; TOPICS.len()];
    for (i, keywords) in TOPICS.iter().enumerate() {
        let mut hits = 0.0_f32;
        for keyword in *keywords {
            if lower.contains(keyword) {
                hits += 1.0;
            }
        }
        vector[i] = hits;
    }
    let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut vector {
            *x /= norm;
        }
    }
    vector
}

const SKILLS: &[(&str, &str)] = &[
    (
        "git-pr-reviewer",
        "Verify GitHub pull requests with evidence checklists",
    ),
    (
        "rust-tester",
        "Run cargo test suites for Rust code and report unit test failures",
    ),
    (
        "deployer",
        "Publish releases and deploy services to production hosting",
    ),
    (
        "memory-archivist",
        "Store and recall long-term agent memories and notes",
    ),
    (
        "net-debugger",
        "Diagnose network failures, timeouts and page latency",
    ),
    (
        "api-auth-guard",
        "Guard HTTP APIs with login tokens, auth checks and secret scanning",
    ),
    (
        "db-migrator",
        "Migrate Postgres database schemas and backfill tables",
    ),
    (
        "chat-summarizer",
        "Turn meeting chats into decisions and action items",
    ),
];

const CASES: &[(&str, &str)] = &[
    ("review a pull request", "git-pr-reviewer"),
    ("check this github merge for issues", "git-pr-reviewer"),
    ("verify the diff evidence before merging", "git-pr-reviewer"),
    ("cargo tests are failing", "rust-tester"),
    ("run the rust unit tests", "rust-tester"),
    ("which unit test broke", "rust-tester"),
    ("ship the new release to production", "deployer"),
    ("publish the hosting rollout", "deployer"),
    ("deploy the service now", "deployer"),
    ("remember what we decided", "memory-archivist"),
    ("recall my stored notes", "memory-archivist"),
    ("archive this for later", "memory-archivist"),
    ("pages load slowly", "net-debugger"),
    ("connection keeps timing out", "net-debugger"),
    ("why is the site offline", "net-debugger"),
    ("check the login tokens", "api-auth-guard"),
    ("scan for leaked secrets", "api-auth-guard"),
    ("secure the api endpoints", "api-auth-guard"),
    ("backfill the postgres tables", "db-migrator"),
    ("migrate the sql schema", "db-migrator"),
    ("alter the database tables", "db-migrator"),
    ("summarize the meeting chat", "chat-summarizer"),
    ("list decisions and action items", "chat-summarizer"),
    ("write the meeting minutes", "chat-summarizer"),
];

#[tokio::test]
async fn skill_semantic_rank_eval() {
    assert!(CASES.len() >= 20, "eval set must hold >= 20 queries");

    let dir = tempfile::tempdir().unwrap();
    for (name, description) in SKILLS {
        let skill_dir = dir.path().join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: \"{description}\"\n---\n\n# {name}\n"),
        )
        .unwrap();
    }

    let mut registry = SkillRegistry::new(vec![dir.path().to_path_buf()]);
    registry.reindex().await.unwrap();
    assert_eq!(registry.len(), SKILLS.len());

    // Offline backfill: vectors from the mocked port, zero network.
    let names: Vec<String> = registry
        .list()
        .into_iter()
        .map(|name| name.to_string())
        .collect();
    for name in &names {
        let description = registry.get(name).unwrap().description.clone();
        registry.set_skill_embedding(name, mock_embed(&skill_embed_text(name, &description)));
    }
    assert_eq!(registry.vector_count(), SKILLS.len());

    let mut hits = 0_usize;
    let mut reciprocal_sum = 0.0_f64;
    for (query, expected) in CASES {
        let ranked = registry.search_with_vector(query, &mock_embed(query), 3);
        match ranked.iter().position(|(_, skill)| skill.name == *expected) {
            Some(pos) => {
                hits += 1;
                reciprocal_sum += 1.0 / (pos as f64 + 1.0);
            }
            None => {
                let top: Vec<&str> = ranked
                    .iter()
                    .map(|(_, skill)| skill.name.as_str())
                    .collect();
                println!("MISS: {query:?} expected {expected}, top-3: {top:?}");
            }
        }
    }

    let total = CASES.len() as f64;
    let recall_at_3 = hits as f64 / total;
    let mrr = reciprocal_sum / total;
    println!("labelled queries: {}", CASES.len());
    println!("Recall@3 = {recall_at_3:.4}");
    println!("MRR = {mrr:.4}");
    assert!(
        recall_at_3 >= 0.8,
        "Recall@3 {recall_at_3:.4} below 0.8 threshold"
    );
}
