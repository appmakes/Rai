pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;

pub fn truncate_output(s: &str) -> String {
    if s.len() <= MAX_OUTPUT_BYTES {
        s.to_string()
    } else {
        let truncated = &s[..MAX_OUTPUT_BYTES];
        format!(
            "{}\n\n[truncated — {} bytes total, showing first {}]",
            truncated,
            s.len(),
            MAX_OUTPUT_BYTES
        )
    }
}
