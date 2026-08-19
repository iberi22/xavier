use code_graph::db::CodeGraphDB;
use code_graph::types::{Language, Symbol, SymbolKind};
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::tempdir;

/// Helper struct to manage project registration and namespace discovery
/// in multi-project workspace environments.
pub struct MultiProjectManager {
    projects: HashMap<String, PathBuf>,
}

impl MultiProjectManager {
    pub fn new() -> Self {
        Self {
            projects: HashMap::new(),
        }
    }

    /// Register a project namespace and its corresponding root path.
    pub fn register_project(&mut self, project_id: &str, root_path: PathBuf) {
        self.projects.insert(project_id.to_string(), root_path);
    }

    /// List all registered project namespaces.
    pub fn list_projects(&self) -> Vec<String> {
        let mut list: Vec<String> = self.projects.keys().cloned().collect();
        list.sort();
        list
    }

    /// Get root path for a registered project namespace.
    pub fn get_project(&self, project_id: &str) -> Option<&PathBuf> {
        self.projects.get(project_id)
    }
}

impl Default for MultiProjectManager {
    fn default() -> Self {
        Self::new()
    }
}

#[test]
fn test_project_registry_management() {
    let mut manager = MultiProjectManager::new();

    let dir_a = tempdir().expect("tempdir a");
    let dir_b = tempdir().expect("tempdir b");

    manager.register_project("frontend-service", dir_a.path().to_path_buf());
    manager.register_project("backend-service", dir_b.path().to_path_buf());

    let projects = manager.list_projects();
    assert_eq!(projects.len(), 2);
    assert_eq!(projects, vec!["backend-service", "frontend-service"]);

    assert_eq!(
        manager.get_project("frontend-service"),
        Some(&dir_a.path().to_path_buf())
    );
    assert_eq!(
        manager.get_project("backend-service"),
        Some(&dir_b.path().to_path_buf())
    );
}

#[test]
fn test_multi_project_namespace_isolation_no_collisions() {
    let db = CodeGraphDB::in_memory().expect("in-memory db");

    let proj_a_id = "project_alpha";
    let proj_b_id = "project_beta";

    // Both projects have identical relative file path and function signature
    let mut sym_a = Symbol {
        id: None,
        stable_id: None,
        name: "initialize_database".to_string(),
        kind: SymbolKind::Function,
        lang: Language::Rust,
        file_path: "src/db.rs".to_string(),
        start_line: 10,
        end_line: 25,
        start_col: 0,
        end_col: 1,
        signature: Some("pub fn initialize_database() -> Result<()>".to_string()),
        parent: None,
        complexity: Some(2.5),
    };

    let mut sym_b = Symbol {
        id: None,
        stable_id: None,
        name: "initialize_database".to_string(),
        kind: SymbolKind::Function,
        lang: Language::Rust,
        file_path: "src/db.rs".to_string(),
        start_line: 10,
        end_line: 25,
        start_col: 0,
        end_col: 1,
        signature: Some("pub fn initialize_database() -> Result<()>".to_string()),
        parent: None,
        complexity: Some(3.0),
    };

    // Calculate project-scoped deterministic symbol IDs
    let stable_id_a = sym_a.deterministic_id(proj_a_id);
    let stable_id_b = sym_b.deterministic_id(proj_b_id);

    // Verify structural hashes are distinct due to project namespace isolation
    assert_ne!(
        stable_id_a, stable_id_b,
        "Symbols with same file and signature in different project namespaces must have distinct stable IDs"
    );

    sym_a.stable_id = Some(stable_id_a.clone());
    sym_b.stable_id = Some(stable_id_b.clone());

    // Insert both symbols into shared SQLite database instance
    db.insert_symbol(&sym_a).expect("insert symbol A");
    db.insert_symbol(&sym_b).expect("insert symbol B");

    // Both symbols must coexist in SQLite without collision
    let query_res = db
        .find_symbols("initialize_database", 10)
        .expect("query symbols");
    assert_eq!(
        query_res.symbols.len(),
        2,
        "Database should store both symbols independently across project namespaces"
    );

    // Verify lookup by stable_id targets exact project symbol
    let fetched_a = db
        .symbol_by_stable_id(&stable_id_a)
        .expect("lookup A")
        .expect("symbol A found");
    let fetched_b = db
        .symbol_by_stable_id(&stable_id_b)
        .expect("lookup B")
        .expect("symbol B found");

    assert_eq!(fetched_a.complexity, Some(2.5));
    assert_eq!(fetched_b.complexity, Some(3.0));
}

#[test]
fn test_cross_project_query_and_filtering() {
    let mut manager = MultiProjectManager::new();

    let dir_payment = tempdir().expect("tempdir payment");
    let dir_auth = tempdir().expect("tempdir auth");

    manager.register_project("payment-gateway", dir_payment.path().to_path_buf());
    manager.register_project("auth-service", dir_auth.path().to_path_buf());

    let db = CodeGraphDB::in_memory().expect("db");

    let mut sym_payment = Symbol {
        id: None,
        stable_id: None,
        name: "ProcessPayment".to_string(),
        kind: SymbolKind::Struct,
        lang: Language::Rust,
        file_path: "src/payment.rs".to_string(),
        start_line: 1,
        end_line: 20,
        start_col: 0,
        end_col: 0,
        signature: Some("pub struct ProcessPayment".to_string()),
        parent: None,
        complexity: Some(1.0),
    };

    let mut sym_auth = Symbol {
        id: None,
        stable_id: None,
        name: "AuthenticateUser".to_string(),
        kind: SymbolKind::Struct,
        lang: Language::Rust,
        file_path: "src/auth.rs".to_string(),
        start_line: 1,
        end_line: 15,
        start_col: 0,
        end_col: 0,
        signature: Some("pub struct AuthenticateUser".to_string()),
        parent: None,
        complexity: Some(1.0),
    };

    sym_payment.stable_id = Some(sym_payment.deterministic_id("payment-gateway"));
    sym_auth.stable_id = Some(sym_auth.deterministic_id("auth-service"));

    db.insert_symbol(&sym_payment).expect("insert payment");
    db.insert_symbol(&sym_auth).expect("insert auth");

    let all_symbols = db.get_all_symbols().expect("get all");
    assert_eq!(all_symbols.len(), 2);

    let active_projects = manager.list_projects();
    assert_eq!(active_projects, vec!["auth-service", "payment-gateway"]);
}
