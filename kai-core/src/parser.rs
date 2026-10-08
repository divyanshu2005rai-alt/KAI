// ============================================================
// parser.rs — Trigger text → structured command
//
// Parses raw captured text like:
//   "@@ask explain recursion"
//   "//coder write a bubble sort in Python"
//   "//fix I wanted to ask if you could send the report"
//
// Into a ParsedCommand with command name + user prompt.
// ============================================================

/// A parsed, structured command extracted from raw trigger text.
#[derive(Debug, Clone)]
pub struct ParsedCommand {
    /// Command name (e.g. "ask", "coder", "fix", "explain")
    pub command: String,
    /// The user's prompt (everything after the command name)
    pub prompt: String,
}

/// Parses raw trigger text into a structured command.
///
/// Returns None if the text is malformed or contains no command.
///
/// # Examples
///
/// ```
/// let parsed = parse_trigger("@@ask explain recursion").unwrap();
/// assert_eq!(parsed.command, "ask");
/// assert_eq!(parsed.prompt, "explain recursion");
/// ```
pub fn parse_trigger(raw: &str) -> Option<ParsedCommand> {
    // Strip the trigger prefix (anything before the first alphanumeric)
    let stripped = raw
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .trim();

    if stripped.is_empty() {
        return None;
    }

    // Split into command + rest
    let mut parts = stripped.splitn(2, |c: char| c.is_whitespace());
    let command = parts.next()?.trim().to_lowercase();
    let prompt = parts.next().unwrap_or("").trim().to_string();

    if command.is_empty() {
        return None;
    }

    Some(ParsedCommand { command, prompt })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_ask() {
        let p = parse_trigger("@@ask explain recursion").unwrap();
        assert_eq!(p.command, "ask");
        assert_eq!(p.prompt, "explain recursion");
    }

    #[test]
    fn test_coder_command() {
        let p = parse_trigger("@@coder write a bubble sort in Python").unwrap();
        assert_eq!(p.command, "coder");
        assert_eq!(p.prompt, "write a bubble sort in Python");
    }

    #[test]
    fn test_fix_command() {
        let p = parse_trigger("@@fix I wanted to ask if you could send the report").unwrap();
        assert_eq!(p.command, "fix");
        assert_eq!(p.prompt, "I wanted to ask if you could send the report");
    }

    #[test]
    fn test_command_only() {
        let p = parse_trigger("@@summarize").unwrap();
        assert_eq!(p.command, "summarize");
        assert_eq!(p.prompt, "");
    }

    #[test]
    fn test_empty() {
        assert!(parse_trigger("@@").is_none());
        assert!(parse_trigger("").is_none());
    }

    #[test]
    fn test_uppercase_command() {
        let p = parse_trigger("@@ASK what is rust").unwrap();
        assert_eq!(p.command, "ask"); // normalized to lowercase
    }
}
