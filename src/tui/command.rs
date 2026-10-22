//! The `!cmd` line: the arguments it runs, and how they are shown back.
//!
//! There is no shell here. The arguments are handed to the OS allowlist
//! gate as a vector, so the quoting is only there to say where one
//! argument ends and the next begins — the same job a shell's quoting
//! does, and nothing more.

/// Split a `!cmd` line into its arguments.
///
/// Whitespace separates the arguments, and a quoted run is one argument:
/// `!echo "hello world"` is two arguments, not three. A single-quoted
/// run is literal. In a double-quoted run a backslash escapes the next
/// character — `\"` is one quote and `\\` is one backslash — the way a
/// shell spells it, so a quoted path is written with doubled
/// backslashes. An unterminated quote runs to the end of the line.
pub(crate) fn split(line: &str) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut current = String::new();
    // An empty quoted argument (`""`) is an argument, so the presence of
    // an argument is tracked separately from its content.
    let mut started = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '"' | '\'' => {
                let quote = c;
                started = true;
                let rest = chars.by_ref();
                while let Some(c) = rest.next() {
                    if c == quote {
                        break;
                    }
                    if quote == '"' && c == '\\' {
                        // A double-quoted run escapes the next character:
                        // `\"` is one quote and `\\` is one backslash.
                        // `quote` spells it the same way, so the shown
                        // command reads back as the same arguments.
                        if let Some(next) = rest.next() {
                            current.push(next);
                        }
                        continue;
                    }
                    current.push(c);
                }
            }
            c => {
                started = true;
                current.push(c);
            }
        }
    }
    if started {
        args.push(current);
    }
    args
}

/// Show the arguments the way they will be run.
///
/// The confirm dialog has to show the command it is about to run. The
/// arguments are shown quoted, so the dialog and the argument vector
/// read the same way, and `split(show(args))` is the argument vector
/// again.
pub(crate) fn show(args: &[String]) -> String {
    args.iter()
        .map(|arg| quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote(arg: &str) -> String {
    if !arg.is_empty()
        && !arg
            .chars()
            .any(|c| c.is_whitespace() || c == '"' || c == '\'' || c == '\\')
    {
        return arg.to_string();
    }
    // A double-quoted run escapes the backslash before the quote, so the
    // argument reads back as the same argument: `split(show(args))` is
    // the argument vector again.
    let escaped = arg.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_separates_the_arguments() {
        assert_eq!(split("whoami"), vec!["whoami".to_string()]);
        assert_eq!(
            split("ls -la /tmp"),
            vec!["ls".to_string(), "-la".to_string(), "/tmp".to_string()]
        );
    }

    #[test]
    fn a_quoted_run_is_one_argument() {
        assert_eq!(
            split("echo \"hello world\""),
            vec!["echo".to_string(), "hello world".to_string()]
        );
        assert_eq!(
            split("echo 'hello world'"),
            vec!["echo".to_string(), "hello world".to_string()]
        );
    }

    #[test]
    fn an_empty_quoted_argument_is_an_argument() {
        assert_eq!(split("echo \"\""), vec!["echo".to_string(), String::new()]);
    }

    #[test]
    fn an_unterminated_quote_runs_to_the_end_of_the_line() {
        assert_eq!(
            split("echo \"hello world"),
            vec!["echo".to_string(), "hello world".to_string()]
        );
    }

    #[test]
    fn a_double_quoted_run_escapes_the_next_character() {
        assert_eq!(
            split(r#"echo "C:\\Users\\me""#),
            vec!["echo".to_string(), r"C:\Users\me".to_string()]
        );
        assert_eq!(
            split(r#"echo "say \"hi\"""#),
            vec!["echo".to_string(), r#"say "hi""#.to_string()]
        );
        // A single-quoted run is literal: there is no escape in it.
        assert_eq!(
            split(r"echo 'C:\Users\me'"),
            vec!["echo".to_string(), r"C:\Users\me".to_string()]
        );
    }

    #[test]
    fn the_shown_command_reads_back_as_the_same_arguments() {
        for args in [
            vec!["echo".to_string(), "hello world".to_string()],
            vec!["echo".to_string(), String::new()],
            vec![r#"say "hi""#.to_string()],
            vec!["it's".to_string()],
            vec!["C:\\Users\\me".to_string()],
        ] {
            assert_eq!(split(&show(&args)), args, "round trip of {args:?}");
        }
    }
}
