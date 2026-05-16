//! Redaction helpers for secrets in diagnostic output.
//!
//! Rule: never expose more than the last 4 characters of any secret, and never
//! let any `sk-` prefix appear in a log-safe representation.

/// Redact a secret string for diagnostic output.
///
/// Returns at most the last 4 characters of `s` preceded by `"****"`:
/// - Empty or very short (≤ 4 chars by Unicode scalar count): `"****"`.
/// - Longer: `"****"` + the last 4 Unicode scalar values of `s`.
///
/// Uses char-safe slicing so multibyte inputs (e.g. accented characters)
/// never cause a byte-boundary panic.
///
/// This ensures that a full `sk-proj-…` API key is reduced to something like
/// `"****1234"` — recognisable enough for user support but containing no
/// sensitive prefix and at most 4 original characters.
pub fn redact_secret(s: &str) -> String {
    const MASK: &str = "****";
    if s.chars().count() <= 4 {
        MASK.to_string()
    } else {
        // Collect the last 4 chars safely (no byte-boundary panic on multibyte input).
        let suffix: String = s
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("{MASK}{suffix}")
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::redact_secret;

    /// TDD: empty string → all-mask.
    #[test]
    fn empty_returns_mask() {
        assert_eq!(redact_secret(""), "****");
    }

    /// TDD: string of exactly 4 chars → all-mask (no originals exposed).
    #[test]
    fn exactly_four_chars_returns_mask() {
        assert_eq!(redact_secret("1234"), "****");
    }

    /// TDD: string of 5 chars → mask + last 4 chars (rule: last 4, not last 1).
    #[test]
    fn five_chars_exposes_four() {
        let result = redact_secret("abcde");
        // last 4 of "abcde" is "bcde"
        assert_eq!(result, "****bcde");
    }

    /// TDD: string of 8 chars → mask + last 4 chars.
    #[test]
    fn eight_chars_exposes_four() {
        let result = redact_secret("abcdefgh");
        assert_eq!(result, "****efgh");
    }

    /// TDD: full sk-proj key → contains no "sk-" and at most 4 original chars.
    #[test]
    fn sk_proj_key_has_no_sk_prefix_and_at_most_four_original_chars() {
        let key = "sk-proj-ABCDEFGHIJKLMNOPQRSTUVWXYZ1234";
        let result = redact_secret(key);

        // Must not contain the sensitive prefix.
        assert!(
            !result.contains("sk-"),
            "redacted key must not contain 'sk-', got: {result}"
        );

        // Only the last 4 chars of the original key can appear in the output.
        // The last 4 chars of key are "1234".
        let last4 = &key[key.len() - 4..];
        assert!(
            result.ends_with(last4),
            "expected suffix {last4:?}, got: {result}"
        );

        // The mask prefix must be present.
        assert!(
            result.starts_with("****"),
            "must start with '****', got: {result}"
        );

        // At most 4 non-mask characters (the trailing suffix).
        let non_mask = result.trim_start_matches('*');
        assert!(
            non_mask.len() <= 4,
            "more than 4 original chars exposed: {non_mask:?}"
        );
    }

    /// TDD: result never exposes more than 4 trailing characters for any length.
    #[test]
    fn never_more_than_four_original_chars() {
        for len in 0_usize..=100 {
            let s: String = "x".repeat(len);
            let result = redact_secret(&s);
            assert!(result.starts_with("****"), "len={len}: {result}");
            let non_mask = result.trim_start_matches('*');
            assert!(
                non_mask.len() <= 4,
                "len={len}: exposed {non_mask_len} chars > 4",
                non_mask_len = non_mask.len()
            );
        }
    }

    /// Issue 4: multibyte input must not panic and must expose ≤ 4 trailing chars.
    ///
    /// "clé-d'accès-é" contains multibyte UTF-8 chars (é = 2 bytes each);
    /// byte-index slicing would panic on non-char boundaries — char-safe code
    /// must handle this correctly.
    #[test]
    fn multibyte_input_no_panic_and_at_most_four_trailing_chars() {
        let s = "clé-d'accès-é";
        // Must not panic.
        let result = redact_secret(s);
        // Must start with the mask.
        assert!(
            result.starts_with("****"),
            "must start with '****', got: {result}"
        );
        // The non-mask suffix must be at most 4 Unicode scalars.
        let suffix: &str = result.trim_start_matches('*');
        let suffix_char_count = suffix.chars().count();
        assert!(
            suffix_char_count <= 4,
            "exposed {suffix_char_count} chars (> 4): {suffix:?}"
        );
        // The suffix must match the last 4 chars of the original string.
        let expected_suffix: String = s
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert_eq!(
            suffix, expected_suffix,
            "suffix must be last 4 chars of input"
        );
    }
}
