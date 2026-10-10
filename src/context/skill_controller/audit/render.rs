//! What a finding and a proposal are allowed to say, and how a proposal is built.

use sha2::{Digest, Sha256};
use url::Url;

/// The longest placement name echoed into a rendered description line.
const NAME_ECHO_MAX: usize = 128;

/// A location digest is the only trace of a link target that reaches a report:
/// an 8-hex-character sha256 prefix, so a key-shaped filename is not echoed.
pub fn target_digest(target: &str) -> String {
    Sha256::digest(target.as_bytes())
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Scheme, host and port only: userinfo, path, query and fragment are dropped,
/// so a credential embedded in a literal never reaches a report.
pub fn redact_url(literal: &str) -> String {
    let Ok(parsed) = Url::parse(literal) else {
        return "an unparsable http literal".into();
    };
    let port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    format!(
        "{}://{}{port}",
        parsed.scheme(),
        parsed.host_str().unwrap_or_default()
    )
}

/// The structured proposal for a missing `description`: the 0-based index the
/// composed line occupies and that line itself. No member byte is carried, so
/// the rendered diff can never quote the skill body.
pub fn description_insertion(text: &str, name: &str) -> (usize, String) {
    let at = text
        .lines()
        .position(|line| line.trim_start().starts_with("name:"))
        .map_or(1, |index| index + 1);
    let placeholder = format!("description: TODO describe when to use {}", echo_name(name));
    (at, placeholder)
}

/// A placement name made safe to render: no control character, no newline and
/// bounded, so a composed line stays one line of frontmatter.
fn echo_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(NAME_ECHO_MAX)
        .collect()
}

/// A unified diff with zero context lines: the file header, the hunk header and
/// the single added line. Assembled here instead of diffed against the member,
/// so a proposal is the audit's own text and never a copy of the skill.
pub fn proposal_diff(added: &str, at: usize) -> String {
    format!(
        "--- a/SKILL.md\n+++ b/SKILL.md\n@@ -{at},0 +{next} @@\n+{added}\n",
        next = at + 1
    )
}
