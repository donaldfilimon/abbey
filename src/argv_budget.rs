//! Shared byte ceilings and UTF-8 prompt truncation for process argv.
//!
//! This pure module can also be tested with `rustc --test src/argv_budget.rs`.
//! That exercises these helpers only; it never substitutes for ./check.sh or
//! native process supervision acceptance.

pub(crate) const MAX_PROCESS_ARG_BYTES: usize = 32 * 1024;
pub(crate) const MAX_PROCESS_TOTAL_ARG_BYTES: usize = 64 * 1024;

pub(crate) fn max_prompt_bytes() -> usize {
    if cfg!(windows) {
        24 * 1024
    } else {
        MAX_PROCESS_ARG_BYTES
    }
}

/// Truncate `s` on a UTF-8 boundary so its byte length is ≤ `max_bytes`.
pub fn truncate_utf8_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let marker = format!(
        "\n\n… [truncated for OS argv limit; kept {max_bytes} of {} bytes]",
        s.len()
    );
    let budget = max_bytes.saturating_sub(marker.len());
    let mut end = budget.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = s[..end].to_string();
    out.push_str(&marker);
    if out.len() > max_bytes {
        let mut e = max_bytes;
        while e > 0 && !out.is_char_boundary(e) {
            e -= 1;
        }
        out.truncate(e);
    }
    out
}

/// Preserve every fixed argument and spend only the remaining aggregate on
/// prompt strings. Invalid or oversized fixed configuration is still refused
/// by the supervisor; it must never be silently rewritten into another action.
pub(crate) fn append_prompt_args(args: &mut Vec<String>, prompts: Vec<String>) {
    let fixed_bytes = args
        .iter()
        .map(String::len)
        .fold(0_usize, usize::saturating_add);
    let mut remaining = MAX_PROCESS_TOTAL_ARG_BYTES.saturating_sub(fixed_bytes);
    for prompt in prompts {
        let text = truncate_utf8_bytes(&prompt, max_prompt_bytes().min(remaining));
        remaining = remaining.saturating_sub(text.len());
        args.push(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertised_prompt_fits_supervisor_argument_ceiling() {
        assert!(max_prompt_bytes() >= 16 * 1024);
        assert!(max_prompt_bytes() <= MAX_PROCESS_ARG_BYTES);
    }

    #[test]
    fn fixed_arguments_consume_the_aggregate_before_unicode_prompts() {
        for fixed in [0, 16 * 1024, 32 * 1024] {
            let prefix = vec!["a".repeat(fixed), "b".repeat(fixed)];
            let mut args = prefix.clone();
            append_prompt_args(&mut args, vec!["界".repeat(40_000); 3]);
            assert_eq!(&args[..2], &prefix);
            assert_eq!(args.len(), 5);
            assert!(args.iter().all(|arg| arg.len() <= MAX_PROCESS_ARG_BYTES));
            assert!(args.iter().map(String::len).sum::<usize>() <= MAX_PROCESS_TOTAL_ARG_BYTES);
        }
    }

    #[test]
    fn invalid_fixed_configuration_is_preserved_for_validation() {
        let fixed = "x".repeat(MAX_PROCESS_TOTAL_ARG_BYTES + 1);
        let mut args = vec![fixed.clone()];
        append_prompt_args(&mut args, vec!["prompt".into()]);
        assert_eq!(args, vec![fixed, String::new()]);
    }

    #[test]
    fn small_arguments_are_preserved_exactly() {
        let mut args = vec!["--model".into(), "literal:model".into()];
        append_prompt_args(&mut args, vec!["a\nb\t界".into(), "second".into()]);
        assert_eq!(args, ["--model", "literal:model", "a\nb\t界", "second"]);
    }

    #[test]
    fn truncation_obeys_tiny_and_unicode_byte_budgets() {
        let input = "界🦀é".repeat(50);
        for cap in 0..=input.len() + 1 {
            assert!(truncate_utf8_bytes(&input, cap).len() <= cap);
        }
        let truncated = truncate_utf8_bytes(&input, 150);
        assert!(truncated.contains("truncated for OS argv limit"));
        assert_eq!(truncate_utf8_bytes(&input, input.len()), input);
    }
}
