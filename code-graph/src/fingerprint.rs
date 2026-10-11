//! Structural fingerprints (`ksm-sfp/1`).
//!
//! Deterministic, tree-sitter based per-function hashes, identifiers and line
//! numbers. Never contains source text. See the `VULN-ATLAS.md` contract
//! (section 1) for the normative definitions.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tree_sitter::{Node, Parser};

use crate::types::Language;

/// Schema id emitted in every fingerprint document.
pub const SCHEMA: &str = "ksm-sfp/1";
/// Coverage value for a successfully parsed file.
pub const COVERAGE_OK: &str = "OK";
/// Coverage value when no parser handles the file.
pub const COVERAGE_SKIPPED: &str = "PARSER_SKIPPED";

/// One call expression in source order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallInfo {
    /// Positional arguments: first identifier per argument, `""` when none.
    pub args: Vec<String>,
    /// Last name segment of the callee (`a->b` -> `b`).
    pub callee: String,
    /// 1-based line of the call.
    pub line: usize,
}

/// One guard condition in source order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardInfo {
    /// `if` | `while` | `for` | `do` | `ternary` | `match`.
    pub kind: String,
    /// 1-based line of the guard node.
    pub line: usize,
    /// Distinct identifiers in the condition, sorted.
    pub vars: Vec<String>,
}

/// One distinct identifier with every line it occurs on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VarInfo {
    /// 1-based lines, one entry per occurrence, ascending.
    pub lines: Vec<usize>,
    /// Identifier text.
    pub name: String,
}

/// Fingerprint of a single function.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionFingerprint {
    pub calls: Vec<CallInfo>,
    pub end_line: usize,
    pub guards: Vec<GuardInfo>,
    /// 3-token window hashes (8 lowercase hex) to counts.
    pub kgrams: BTreeMap<String, u64>,
    /// 64 min-hash values over the kgram keys.
    pub minhash: Vec<u32>,
    pub name: String,
    /// Number of named nodes in the function subtree.
    pub node_count: usize,
    pub start_line: usize,
    pub vars: Vec<VarInfo>,
}

/// Fingerprint of a whole file (`ksm-sfp/1`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileFingerprint {
    pub coverage: String,
    pub file: String,
    pub functions: Vec<FunctionFingerprint>,
    pub language: String,
    pub schema: String,
    pub tool: String,
}

/// `code-graph <crate version>`, e.g. `code-graph 0.2.1`.
pub fn tool_string() -> String {
    format!("code-graph {}", env!("CARGO_PKG_VERSION"))
}

/// Resolve a `--lang` value to a [`Language`]. Unknown names map to
/// [`Language::Unknown`] (caller emits `PARSER_SKIPPED`).
pub fn resolve_language(name: &str) -> Language {
    match name.trim().to_ascii_lowercase().as_str() {
        "c" | "h" => Language::C,
        "cpp" | "c++" | "cc" | "cxx" | "hpp" => Language::Cpp,
        "rust" | "rs" => Language::Rust,
        "python" | "py" => Language::Python,
        "go" => Language::Go,
        "java" => Language::Java,
        "typescript" | "ts" => Language::TypeScript,
        "javascript" | "js" => Language::JavaScript,
        _ => Language::Unknown,
    }
}

/// Parse a `--ranges` spec (`10-50,80-120`) into 1-based inclusive ranges.
pub fn parse_ranges(spec: &str) -> Result<Vec<(usize, usize)>, String> {
    let mut out = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        let (a, b) = part
            .split_once('-')
            .ok_or_else(|| format!("bad range {part:?}, want a-b"))?;
        let a: usize = a
            .trim()
            .parse()
            .map_err(|_| format!("bad range {part:?}, want a-b"))?;
        let b: usize = b
            .trim()
            .parse()
            .map_err(|_| format!("bad range {part:?}, want a-b"))?;
        if a < 1 || b < a {
            return Err(format!("bad range {part:?}, want 1 <= a <= b"));
        }
        out.push((a, b));
    }
    if out.is_empty() {
        return Err("empty --ranges".to_string());
    }
    Ok(out)
}

/// Fingerprint `source` as `lang`. `file` is the path as given (recorded
/// verbatim). `ranges` filters to functions whose `[start_line, end_line]`
/// overlaps at least one range; empty means every function.
pub fn fingerprint_source(
    source: &str,
    lang: Language,
    file: &str,
    ranges: &[(usize, usize)],
) -> FileFingerprint {
    let skipped = || FileFingerprint {
        coverage: COVERAGE_SKIPPED.to_string(),
        file: file.to_string(),
        functions: Vec::new(),
        language: lang.as_str().to_string(),
        schema: SCHEMA.to_string(),
        tool: tool_string(),
    };
    let mut parser = match parser_for(&lang) {
        Some(p) => p,
        None => return skipped(),
    };
    let neutral;
    let source = if matches!(lang, Language::C | Language::Cpp) {
        neutral = neutralize_preprocessor(source);
        neutral.as_str()
    } else {
        source
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t,
        None => return skipped(),
    };
    let root = tree.root_node();
    let src = source.as_bytes();
    let kinds = function_kinds(&lang);
    let mut found: Vec<(Node, String)> = Vec::new();
    collect_functions(root, None, &kinds, src, &mut found);
    if found.is_empty() && root.has_error() && !source.trim().is_empty() {
        return skipped();
    }
    let mut functions = Vec::new();
    for (node, name) in found {
        let start_line = node.start_position().row + 1;
        let end_line = node.end_position().row + 1;
        if !ranges.is_empty()
            && !ranges
                .iter()
                .any(|(a, b)| start_line <= *b && *a <= end_line)
        {
            continue;
        }
        functions.push(fingerprint_function(node, &name, source));
    }
    FileFingerprint {
        coverage: COVERAGE_OK.to_string(),
        file: file.to_string(),
        functions,
        language: lang.as_str().to_string(),
        schema: SCHEMA.to_string(),
        tool: tool_string(),
    }
}

/// Serialise a fingerprint with sorted keys (byte-stable).
pub fn to_json_sorted(fp: &FileFingerprint) -> String {
    serde_json::to_string_pretty(fp).unwrap_or_else(|_| "{}".to_string())
}

/// Blank C/C++ preprocessor directives and keep one branch per conditional,
/// preserving line numbers. tree-sitter parses unpreprocessed code, and a
/// `#if` that splits an `else if` chain otherwise breaks the enclosing
/// function into fake definitions. The first branch is kept, except for
/// `#if 0`, where the `#else` branch is kept. Continuation lines (`\`) of a
/// directive are blanked too.
pub fn neutralize_preprocessor(source: &str) -> String {
    // Per open conditional: (parent active, this branch active, a branch taken).
    let mut stack: Vec<(bool, bool, bool)> = Vec::new();
    let mut out = String::with_capacity(source.len());
    let mut continuation = false;
    for line in source.split_inclusive('\n') {
        let (body, eol) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let trimmed = body.trim_start();
        let active = stack
            .last()
            .map_or(true, |&(parent, branch, _)| parent && branch);
        if continuation {
            continuation = body.trim_end().ends_with('\\');
            out.push_str(eol);
            continue;
        }
        if let Some(directive) = trimmed.strip_prefix('#') {
            continuation = body.trim_end().ends_with('\\');
            let directive = directive.trim_start();
            let word: String = directive
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect();
            match word.as_str() {
                "if" | "ifdef" | "ifndef" => {
                    let rest = directive[word.len()..].trim();
                    let taken = !(word == "if" && rest == "0");
                    stack.push((active, taken, taken));
                }
                "elif" | "elifdef" | "elifndef" => {
                    if let Some(top) = stack.last_mut() {
                        let take = !top.2;
                        top.1 = take;
                        top.2 |= take;
                    }
                }
                "else" => {
                    if let Some(top) = stack.last_mut() {
                        top.1 = !top.2;
                        top.2 = true;
                    }
                }
                "endif" => {
                    stack.pop();
                }
                _ => {}
            }
            out.push_str(eol);
            continue;
        }
        if active {
            out.push_str(body);
        }
        out.push_str(eol);
    }
    out
}

/// Hash rule for one token window: first 8 hex chars of the SHA-256 of the
/// tokens joined by U+001F.
pub fn kgram_key(tokens: &[&str]) -> String {
    let joined = tokens.join("\x1f");
    let digest = Sha256::digest(joined.as_bytes());
    hex::encode(&digest[..4])
}

fn parser_for(lang: &Language) -> Option<Parser> {
    let mut parser = Parser::new();
    let set = match lang {
        Language::C => parser.set_language(&tree_sitter_c::LANGUAGE.into()),
        Language::Cpp => parser.set_language(&tree_sitter_cpp::LANGUAGE.into()),
        Language::Rust => parser.set_language(&tree_sitter_rust::LANGUAGE.into()),
        Language::Python => parser.set_language(&tree_sitter_python::LANGUAGE.into()),
        Language::Go => parser.set_language(&tree_sitter_go::LANGUAGE.into()),
        Language::Java => parser.set_language(&tree_sitter_java::LANGUAGE.into()),
        Language::TypeScript | Language::JavaScript => {
            parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        }
        Language::Other(_) | Language::Unknown => return None,
    };
    if set.is_err() {
        return None;
    }
    Some(parser)
}

fn function_kinds(lang: &Language) -> Vec<&'static str> {
    match lang {
        Language::C | Language::Cpp => vec!["function_definition"],
        Language::Rust => vec!["function_item"],
        Language::Python => vec!["function_definition"],
        Language::Go => vec!["function_declaration", "method_declaration"],
        Language::Java => vec!["method_declaration", "constructor_declaration"],
        Language::TypeScript | Language::JavaScript => vec![
            "function_declaration",
            "method_definition",
            "arrow_function",
            "function_expression",
        ],
        Language::Other(_) | Language::Unknown => vec![],
    }
}

fn collect_functions<'a>(
    node: Node<'a>,
    parent: Option<Node<'a>>,
    kinds: &[&str],
    src: &[u8],
    out: &mut Vec<(Node<'a>, String)>,
) {
    if node.is_named() && kinds.contains(&node.kind()) {
        if let Some(name) = function_name(node, parent, src) {
            // A keyword "name" means tree-sitter misread a statement as a
            // K&R definition after an unparsable region; it is not a function.
            if !matches!(
                name.as_str(),
                "if" | "else" | "while" | "for" | "do" | "switch" | "return" | "sizeof"
            ) {
                out.push((node, name));
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_functions(child, Some(node), kinds, src, out);
    }
}

fn function_name(node: Node, parent: Option<Node>, src: &[u8]) -> Option<String> {
    match node.kind() {
        "arrow_function" | "function_expression" => {
            let binder = parent?;
            match binder.kind() {
                "variable_declarator" => binder
                    .child_by_field_name("name")?
                    .utf8_text(src)
                    .ok()
                    .map(|s| s.to_string()),
                "assignment_expression" => {
                    let left = binder.child_by_field_name("left")?;
                    non_empty(last_ident(left.utf8_text(src).unwrap_or("")))
                }
                "pair" => {
                    let key = binder.child_by_field_name("key")?;
                    non_empty(last_ident(key.utf8_text(src).unwrap_or("")))
                }
                _ => None,
            }
        }
        _ => {
            if let Some(name) = node.child_by_field_name("name") {
                return name.utf8_text(src).ok().map(|s| s.to_string());
            }
            // C/C++: name lives inside the declarator subtree.
            let declarator = node.child_by_field_name("declarator")?;
            find_first_identifier_text(declarator, src)
        }
    }
}

fn non_empty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn find_first_identifier_text(node: Node, src: &[u8]) -> Option<String> {
    let mut stack = vec![node];
    while let Some(next) = stack.pop() {
        if next.is_named() && next.kind() == "identifier" {
            if let Ok(text) = next.utf8_text(src) {
                return Some(text.to_string());
            }
        }
        let mut cursor = next.walk();
        let mut children: Vec<Node> = next.children(&mut cursor).collect();
        children.reverse();
        stack.extend(children);
    }
    None
}

/// Last identifier segment of a callee (`a->b` -> `b`, `a::b::c` -> `c`).
fn last_ident(text: &str) -> String {
    let mut last = String::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
        } else if !current.is_empty() {
            last = std::mem::take(&mut current);
        }
    }
    if !current.is_empty() {
        last = current;
    }
    last
}

fn is_ident_kind(kind: &str) -> bool {
    kind == "identifier" || kind.ends_with("_identifier")
}

fn is_comment_kind(kind: &str) -> bool {
    kind.contains("comment")
}

fn is_num_kind(kind: &str, text: &str) -> bool {
    matches!(
        kind,
        "number"
            | "number_literal"
            | "integer"
            | "float"
            | "integer_literal"
            | "float_literal"
            | "int_literal"
            | "decimal_integer_literal"
            | "hex_integer_literal"
            | "octal_integer_literal"
            | "binary_integer_literal"
            | "decimal_floating_point_literal"
            | "hex_floating_point_literal"
    ) || (kind.ends_with("_literal")
        && text
            .trim_start()
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit()))
}

fn is_str_kind(kind: &str, text: &str) -> bool {
    matches!(
        kind,
        "string"
            | "string_literal"
            | "raw_string_literal"
            | "interpreted_string_literal"
            | "char_literal"
            | "character_literal"
            | "rune_literal"
            | "string_fragment"
    ) || kind.contains("string")
        || (kind.ends_with("_literal")
            && text
                .trim_start()
                .chars()
                .next()
                .is_some_and(|c| matches!(c, '"' | '\'' | '`')))
}

fn is_call_kind(kind: &str) -> bool {
    matches!(kind, "call_expression" | "call" | "method_invocation")
}

fn is_op_kind(kind: &str) -> bool {
    kind.contains("binary")
        || kind.contains("assign")
        || kind.contains("update")
        || kind == "comparison_operator"
        || kind == "boolean_operator"
}

fn guard_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "if_statement" | "if_expression" => Some("if"),
        "while_statement" | "while_expression" => Some("while"),
        "for_statement" | "for_expression" | "enhanced_for_statement" => Some("for"),
        "do_statement" => Some("do"),
        "conditional_expression" | "ternary_expression" => Some("ternary"),
        "match_expression" => Some("match"),
        _ => None,
    }
}

fn guard_condition(node: Node) -> Option<Node> {
    if let Some(condition) = node.child_by_field_name("condition") {
        return Some(condition);
    }
    if let Some(test) = node.child_by_field_name("test") {
        return Some(test);
    }
    match node.kind() {
        "match_expression" | "for_expression" => node.child_by_field_name("value"),
        "for_statement" | "enhanced_for_statement" => {
            if let Some(value) = node.child_by_field_name("value") {
                return Some(value);
            }
            if let Some(right) = node.child_by_field_name("right") {
                return Some(right);
            }
            // Go: condition lives on the nested for_clause.
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "for_clause" {
                    if let Some(condition) = child.child_by_field_name("condition") {
                        return Some(condition);
                    }
                }
            }
            None
        }
        "conditional_expression" | "ternary_expression" => {
            // Python conditional_expression has no fields: (body, condition,
            // alternative) in order.
            let mut cursor = node.walk();
            let second = node.children(&mut cursor).filter(|c| c.is_named()).nth(1);
            second
        }
        _ => None,
    }
}

fn operator_text(node: Node, src: &[u8]) -> String {
    if let Some(op) = node.child_by_field_name("operator") {
        if let Ok(text) = op.utf8_text(src) {
            return text.to_string();
        }
    }
    let mut out = String::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            if let Ok(text) = child.utf8_text(src) {
                out.push_str(text);
            }
        }
    }
    out
}

fn first_ident_text(node: Node, src: &[u8]) -> String {
    let mut stack = vec![node];
    while let Some(next) = stack.pop() {
        if next.is_named() && is_ident_kind(next.kind()) {
            if let Ok(text) = next.utf8_text(src) {
                return text.to_string();
            }
        }
        let mut cursor = next.walk();
        let mut children: Vec<Node> = Vec::new();
        for child in next.children(&mut cursor) {
            if child.is_named() {
                children.push(child);
            }
        }
        children.reverse();
        stack.extend(children);
    }
    String::new()
}

fn collect_condition_vars(node: Node, src: &[u8], vars: &mut BTreeSet<String>) {
    let mut stack = vec![node];
    while let Some(next) = stack.pop() {
        if !next.is_named() {
            continue;
        }
        if is_comment_kind(next.kind()) {
            continue;
        }
        if is_ident_kind(next.kind()) {
            if let Ok(text) = next.utf8_text(src) {
                vars.insert(text.to_string());
            }
            continue;
        }
        let mut cursor = next.walk();
        for child in next.children(&mut cursor) {
            stack.push(child);
        }
    }
}

fn count_named_nodes(root: Node) -> usize {
    let mut count = 0;
    let mut stack = vec![root];
    while let Some(next) = stack.pop() {
        if next.is_named() {
            count += 1;
        }
        let mut cursor = next.walk();
        for child in next.children(&mut cursor) {
            stack.push(child);
        }
    }
    count
}

#[derive(Default)]
struct FnWalk {
    tokens: Vec<String>,
    calls: Vec<CallInfo>,
    guards: Vec<GuardInfo>,
    vars: BTreeMap<String, Vec<usize>>,
}

fn walk_function(node: Node, src: &[u8], acc: &mut FnWalk) {
    if !node.is_named() {
        return;
    }
    let kind = node.kind();
    if is_comment_kind(kind) {
        return;
    }
    let line = node.start_position().row + 1;
    if let Some(gk) = guard_kind(kind) {
        let mut vars = BTreeSet::new();
        if let Some(condition) = guard_condition(node) {
            collect_condition_vars(condition, src, &mut vars);
        }
        acc.guards.push(GuardInfo {
            kind: gk.to_string(),
            line,
            vars: vars.into_iter().collect(),
        });
    }
    if is_call_kind(kind) {
        let callee_node = node
            .child_by_field_name("function")
            .or_else(|| node.child_by_field_name("name"));
        let callee = callee_node
            .and_then(|n| n.utf8_text(src).ok())
            .map(last_ident)
            .unwrap_or_default();
        let mut args = Vec::new();
        let args_node = node.child_by_field_name("arguments");
        if let Some(list) = args_node {
            let mut cursor = list.walk();
            for arg in list.children(&mut cursor) {
                if arg.is_named() {
                    args.push(first_ident_text(arg, src));
                }
            }
        }
        acc.calls.push(CallInfo {
            args,
            callee: callee.clone(),
            line,
        });
        acc.tokens.push(format!("call:{callee}"));
        // Walk everything except the callee subtree (function/name/object).
        let mut skip = Vec::new();
        for field in ["function", "name", "object"] {
            if let Some(n) = node.child_by_field_name(field) {
                skip.push((n.start_byte(), n.end_byte()));
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named() && !skip.contains(&(child.start_byte(), child.end_byte())) {
                walk_function(child, src, acc);
            }
        }
        return;
    }
    let text = node.utf8_text(src).unwrap_or("");
    if is_ident_kind(kind) {
        acc.tokens.push("id".to_string());
        acc.vars.entry(text.to_string()).or_default().push(line);
        return;
    }
    if is_num_kind(kind, text) {
        acc.tokens.push("num".to_string());
        return;
    }
    if is_str_kind(kind, text) {
        acc.tokens.push("str".to_string());
        return;
    }
    if is_op_kind(kind) {
        acc.tokens
            .push(format!("{kind}:{}", operator_text(node, src)));
    } else {
        acc.tokens.push(kind.to_string());
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_function(child, src, acc);
    }
}

fn kgrams_for(tokens: &[String]) -> BTreeMap<String, u64> {
    let mut kgrams = BTreeMap::new();
    if tokens.is_empty() {
        return kgrams;
    }
    if tokens.len() < 3 {
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        *kgrams.entry(kgram_key(&refs)).or_insert(0) += 1;
        return kgrams;
    }
    for window in tokens.windows(3) {
        let refs: Vec<&str> = window.iter().map(String::as_str).collect();
        *kgrams.entry(kgram_key(&refs)).or_insert(0) += 1;
    }
    kgrams
}

fn minhash_for(kgrams: &BTreeMap<String, u64>) -> Vec<u32> {
    if kgrams.is_empty() {
        return vec![u32::MAX; 64];
    }
    (0..64u32)
        .map(|i| {
            kgrams
                .keys()
                .map(|k| {
                    let digest = Sha256::digest(format!("{i}:{k}").as_bytes());
                    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
                })
                .min()
                .unwrap_or(u32::MAX)
        })
        .collect()
}

fn fingerprint_function(node: Node, name: &str, source: &str) -> FunctionFingerprint {
    let src = source.as_bytes();
    let node_count = count_named_nodes(node);
    let mut acc = FnWalk::default();
    walk_function(node, src, &mut acc);
    let kgrams = kgrams_for(&acc.tokens);
    let minhash = minhash_for(&kgrams);
    let vars = acc
        .vars
        .into_iter()
        .map(|(name, mut lines)| {
            lines.sort_unstable();
            VarInfo { lines, name }
        })
        .collect();
    FunctionFingerprint {
        calls: acc.calls,
        end_line: node.end_position().row + 1,
        guards: acc.guards,
        kgrams,
        minhash,
        name: name.to_string(),
        node_count,
        start_line: node.start_position().row + 1,
        vars,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kgram_key_uses_unit_separator_join() {
        // Contract rule, pinned: sha256("call:memcpy\x1fid\x1fnum")[:8].
        assert_eq!(kgram_key(&["call:memcpy", "id", "num"]), "5306d8d4");
    }

    #[test]
    fn last_ident_takes_final_segment() {
        assert_eq!(last_ident("a->b"), "b");
        assert_eq!(last_ident("a::b::c"), "c");
        assert_eq!(last_ident("obj.m"), "m");
        assert_eq!(last_ident("free"), "free");
        assert_eq!(last_ident("(*fp)"), "fp");
    }

    #[test]
    fn ranges_parse_and_reject() {
        assert_eq!(parse_ranges("10-50,80-120"), Ok(vec![(10, 50), (80, 120)]));
        assert!(parse_ranges("10").is_err());
        assert!(parse_ranges("50-10").is_err());
        assert!(parse_ranges("0-10").is_err());
        assert!(parse_ranges("").is_err());
    }
}
