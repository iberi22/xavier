//! Secret import primitives: a value read from a stream, and `.env` parsing.
//!
//! Contract for EP-V (T1a): a value never comes from the process argument list
//! and is never written to a terminal or a log. [`parse_env_reader`] only builds
//! the inert data of an [`EnvImportPlan`] — storing is a later, separate call, so
//! a dry run cannot write. Values live in [`SecretValue`], whose `Debug`,
//! `Display` and `Serialize` impls emit [`REDACTED`], so neither `{:?}` nor
//! `serde_json::to_string` can leak one. Diagnostics carry names and line
//! numbers only, never line text, since line text is where the value is. The
//! reader is always the caller's: no file is opened, no process env is read.

use std::fmt;
use std::io::BufRead;

use serde::Serialize;
use thiserror::Error;

/// Placeholder emitted in place of a value by every formatting impl.
pub const REDACTED: &str = "[REDACTED]";

/// A secret value whose `Debug`, `Display` and `Serialize` output is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(String);

impl SecretValue {
    /// New.
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// Borrow the plaintext for one use, e.g. to hand it to the vault.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl fmt::Display for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl Serialize for SecretValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(REDACTED)
    }
}

/// One `NAME=value` assignment planned for the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnvEntry {
    name: String,
    value: SecretValue,
    line: usize,
}

impl EnvEntry {
    /// New.
    pub fn new(name: String, value: SecretValue, line: usize) -> Self {
        Self { name, value, line }
    }

    /// The name as written in the input.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Borrow the plaintext for one use, e.g. to hand it to the vault.
    pub fn expose_value(&self) -> &str {
        self.value.expose()
    }

    /// 1-based line where the assignment was read.
    pub fn line(&self) -> usize {
        self.line
    }
}

/// An ordered, inert list of assignments. Applying it is the caller's job.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct EnvImportPlan {
    entries: Vec<EnvEntry>,
}

impl EnvImportPlan {
    /// New.
    pub fn new(entries: Vec<EnvEntry>) -> Self {
        Self { entries }
    }

    /// Assignments in input order.
    pub fn entries(&self) -> &[EnvEntry] {
        &self.entries
    }

    /// Number of planned assignments.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Why an import input could not be turned into a plan; no input text.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ImportEnvError {
    /// The stream ended without a value.
    #[error("no value on the input stream")]
    EmptyValue,
    /// An assignment had nothing before the `=`.
    #[error("line {line}: missing name before '='")]
    MissingName { line: usize },
    /// A line was not `NAME=value` with a shell-style name.
    #[error("line {line}: not a NAME=value assignment with a valid name")]
    MalformedAssignment { line: usize },
    /// An assignment carried an empty value.
    #[error("line {line}: entry '{name}' has an empty value")]
    EmptyAssignmentValue { line: usize, name: String },
    /// A name repeated in the same input, reported instead of resolved.
    #[error("line {line}: entry '{name}' already defined on line {first_line}")]
    DuplicateName {
        name: String,
        line: usize,
        first_line: usize,
    },
    /// The input could not be read; only the message is kept.
    #[error("cannot read input: {0}")]
    Io(String),
}

/// Read one secret value from `reader` until EOF: exactly one trailing line
/// terminator (`\n` or `\r\n`) is removed, every other space stays part of the
/// value, and an empty result is rejected.
pub fn read_value_from_reader<R: BufRead>(reader: &mut R) -> Result<SecretValue, ImportEnvError> {
    let mut raw = String::new();
    reader.read_to_string(&mut raw).map_err(io_error)?;
    let value = match raw.strip_suffix('\n') {
        Some(head) => head.strip_suffix('\r').unwrap_or(head),
        None => raw.as_str(),
    };
    if value.is_empty() {
        return Err(ImportEnvError::EmptyValue);
    }
    Ok(SecretValue::new(value.to_string()))
}

/// Parse `.env` text into an ordered plan, without touching the vault. Blank
/// lines and `#` comment lines are skipped; an `export ` prefix and surrounding
/// whitespace are dropped; a value may be wrapped in matching single or double
/// quotes, kept verbatim inside. Inline comments are not stripped, because a
/// secret may contain `#` or spaces. A repeated name is reported rather than
/// resolved: silently keeping one of the two decides a value in silence.
pub fn parse_env_reader<R: BufRead>(reader: &mut R) -> Result<EnvImportPlan, ImportEnvError> {
    let mut entries: Vec<EnvEntry> = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line_no = index + 1;
        let line = line.map_err(io_error)?;
        let Some(assignment) = assignment_body(&line) else {
            continue;
        };
        let Some((name, raw_value)) = assignment.split_once('=') else {
            return Err(ImportEnvError::MalformedAssignment { line: line_no });
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(ImportEnvError::MissingName { line: line_no });
        }
        if !is_valid_name(name) {
            return Err(ImportEnvError::MalformedAssignment { line: line_no });
        }
        let value = unquote(raw_value.trim());
        if value.is_empty() {
            return Err(ImportEnvError::EmptyAssignmentValue {
                line: line_no,
                name: name.to_string(),
            });
        }
        // Linear scan: a handful of entries, and no name is stored twice.
        if let Some(previous) = entries.iter().find(|entry| entry.name == name) {
            return Err(ImportEnvError::DuplicateName {
                name: name.to_string(),
                line: line_no,
                first_line: previous.line,
            });
        }
        entries.push(EnvEntry::new(
            name.to_string(),
            SecretValue::new(value.to_string()),
            line_no,
        ));
    }
    Ok(EnvImportPlan::new(entries))
}

/// Reduce a read failure to its message, which never holds the value read.
fn io_error(err: std::io::Error) -> ImportEnvError {
    ImportEnvError::Io(err.to_string())
}

/// Trim a line and drop a leading `export `, or `None` for blank/comment lines.
fn assignment_body(line: &str) -> Option<&str> {
    let body = line.trim();
    if body.is_empty() || body.starts_with('#') {
        return None;
    }
    // `export` is a keyword only when whitespace follows it.
    match body.strip_prefix("export") {
        Some(rest) if rest.starts_with(|c: char| c.is_whitespace()) => Some(rest.trim_start()),
        _ => Some(body),
    }
}

/// Drop matching surrounding quotes, verbatim inside; keep an unmatched quote.
fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

/// A name must be a shell identifier, so a typo is reported, not stored.
fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let starts_well = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_');
    starts_well && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use super::*;

    fn reader(input: &str) -> BufReader<&[u8]> {
        BufReader::new(input.as_bytes())
    }

    fn assert_value_eq(actual: &str, expected: &str) {
        assert!(actual == expected, "parsed value differs from expected");
    }

    #[test]
    fn test_read_value_from_stdin_strips_only_newline() {
        for (raw, expected) in [
            ("synthetic-value\n", "synthetic-value"),
            ("synthetic-value", "synthetic-value"),
            ("synthetic-value\r\n", "synthetic-value"),
            ("synthetic-value\n\n", "synthetic-value\n"),
        ] {
            let val = read_value_from_reader(&mut reader(raw)).expect("value read");
            assert_value_eq(val.expose(), expected);
        }
    }

    #[test]
    fn test_read_value_from_stdin_rejects_empty() {
        for raw in ["", "\n"] {
            let err = read_value_from_reader(&mut reader(raw)).expect_err("empty rejected");
            assert_eq!(err, ImportEnvError::EmptyValue);
        }
    }

    #[test]
    fn test_read_value_from_stdin_accepts_interior_and_trailing_spaces() {
        let val = read_value_from_reader(&mut reader("  spaced  value  \n")).expect("value read");
        assert_value_eq(val.expose(), "  spaced  value  ");
    }

    #[test]
    fn test_parse_env_file_basic_and_comments() {
        let input = concat!(
            "# leading comment\n",
            "CANARY_ALPHA=synthetic-alpha\n\n",
            "   # indented comment\n",
            "CANARY_BETA=\"synthetic beta\"\n",
        );
        let plan = parse_env_reader(&mut reader(input)).expect("plan parses");
        let entries = plan.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name(), "CANARY_ALPHA");
        assert_eq!(entries[0].line(), 2);
        assert_value_eq(entries[0].expose_value(), "synthetic-alpha");
        assert_eq!(entries[1].name(), "CANARY_BETA");
        assert_eq!(entries[1].line(), 5);
        assert_value_eq(entries[1].expose_value(), "synthetic beta");
    }

    #[test]
    fn test_parse_env_file_export_prefix_and_quotes() {
        let input = concat!(
            "export CANARY_GAMMA=synthetic-gamma\n",
            "  export   CANARY_DELTA='  padded  '\n",
            "CANARY_EPSILON=\"hash # stays\"\n",
            "CANARY_ZETA=hash # stays too\n",
        );
        let cases = [
            ("CANARY_GAMMA", "synthetic-gamma"),
            ("CANARY_DELTA", "  padded  "),
            ("CANARY_EPSILON", "hash # stays"),
            ("CANARY_ZETA", "hash # stays too"),
        ];
        let plan = parse_env_reader(&mut reader(input)).expect("plan parses");
        assert_eq!(plan.len(), cases.len());
        for (entry, (name, expected)) in plan.entries().iter().zip(cases) {
            assert_eq!(entry.name(), name);
            assert_value_eq(entry.expose_value(), expected);
        }
    }

    #[test]
    fn test_parse_env_file_reports_duplicate_keys() {
        let err = parse_env_reader(&mut reader("CANARY_THETA=first\nCANARY_THETA=second\n"))
            .expect_err("duplicate is reported");
        assert_eq!(
            err,
            ImportEnvError::DuplicateName {
                name: "CANARY_THETA".to_string(),
                line: 2,
                first_line: 1,
            }
        );
    }

    #[test]
    fn test_parse_env_file_rejects_broken_assignments() {
        let cases = [
            (
                "CANARY_IOTA value\n",
                ImportEnvError::MalformedAssignment { line: 1 },
            ),
            ("=orphan\n", ImportEnvError::MissingName { line: 1 }),
            (
                "CANARY-KAPPA=value\n",
                ImportEnvError::MalformedAssignment { line: 1 },
            ),
            (
                "CANARY_LAMBDA=\n",
                ImportEnvError::EmptyAssignmentValue {
                    line: 1,
                    name: "CANARY_LAMBDA".to_string(),
                },
            ),
        ];
        for (raw, expected) in cases {
            let err = parse_env_reader(&mut reader(raw)).expect_err("rejected");
            assert_eq!(err, expected);
        }
    }

    #[test]
    fn test_parse_env_file_does_not_log_values() {
        let canary = "canary-never-printed-anywhere";
        let plan = parse_env_reader(&mut reader(&format!("CANARY_MU={canary}\n"))).expect("plan");
        assert_value_eq(plan.entries()[0].expose_value(), canary);
        assert!(!format!("{plan:?}").contains(canary), "Debug leaked value");
        assert!(
            !serde_json::to_string(&plan).expect("json").contains(canary),
            "JSON leaked value"
        );
        let err = parse_env_reader(&mut reader(&format!("CANARY_NU bad {canary}\n")))
            .expect_err("rejected");
        assert!(
            !format!("{err} {err:?}").contains(canary),
            "error leaked value"
        );
    }
}
