//! The part of ferx's model grammar that the GUI parser must agree with exactly.
//!
//! Measured against `ferx_model_validate` (ferx 0.4.0.9000): a section header is
//! accepted with a trailing `# comment`, `// comment`, a tab before the comment,
//! or `#comment` with no space; it is rejected (`E_MISSING_BLOCK`) when there is
//! whitespace inside the brackets, e.g. `[ parameters ]`.

/// The section name of a header line, or None when the line is not a header ferx
/// would accept. A header is a trimmed line beginning with `[`; the name is the
/// text up to the first `]` and has no inner whitespace; only whitespace, a `#`
/// comment or a `//` comment may follow the `]`.
pub fn section_header(line: &str) -> Option<&str> {
    let t = line.trim();
    let rest = t.strip_prefix('[')?;
    let close = rest.find(']')?;
    let name = &rest[..close];
    if name.is_empty() || name.chars().any(char::is_whitespace) {
        return None;
    }
    let after = rest[close + 1..].trim_start();
    if after.is_empty() || after.starts_with('#') || after.starts_with("//") {
        Some(name)
    } else {
        None
    }
}

/// True for a line that looks like a header (`[...` with a closing `]`) but that
/// ferx will reject, such as `[ parameters ]`. Used for an editor hint.
#[allow(dead_code)] // editor hint hook
pub fn is_rejected_header(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('[') && t.contains(']') && section_header(t).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_header_strips_comments_like_ferx() {
        for l in ["[parameters]", "[parameters]  # main", "[parameters]  // main",
                  "[parameters]\t# main", "[parameters]#main", "  [parameters]  "] {
            assert_eq!(section_header(l), Some("parameters"), "{l:?}");
        }
        for l in ["[ parameters ]", "[parameters] junk", "[]", "parameters]", "[parameters"] {
            assert_eq!(section_header(l), None, "{l:?}");
        }
        assert!(is_rejected_header("[ parameters ]"));
        assert!(!is_rejected_header("[parameters]"));
    }
}
