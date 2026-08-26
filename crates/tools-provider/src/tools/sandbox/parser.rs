//! Lexical parsing and normalization for shell sandbox decisions.
//!
//! This deliberately does not implement a shell. It recognizes constructs
//! relevant to the execution boundary and produces deterministic segments.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellOperator {
    Pipe,
    Sequence,
    And,
    Or,
    Background,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSegment {
    pub program: String,
    pub args: Vec<String>,
}

impl ShellSegment {
    pub fn normalized(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedShellCommand {
    pub segments: Vec<ShellSegment>,
    pub operators: Vec<ShellOperator>,
    pub has_environment_expansion: bool,
}

impl ParsedShellCommand {
    pub fn normalized(&self) -> String {
        let mut result = String::new();
        for (index, segment) in self.segments.iter().enumerate() {
            if index > 0 {
                result.push_str(match self.operators[index - 1] {
                    ShellOperator::Pipe => " | ",
                    ShellOperator::Sequence => " ; ",
                    ShellOperator::And => " && ",
                    ShellOperator::Or => " || ",
                    ShellOperator::Background => " & ",
                });
            }
            result.push_str(&segment.normalized());
        }
        result
    }

    pub fn has_non_pipe_operator(&self) -> bool {
        self.operators
            .iter()
            .any(|operator| !matches!(operator, ShellOperator::Pipe))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellParseError {
    EmptyCommand,
    UnterminatedSingleQuote,
    UnterminatedDoubleQuote,
    TrailingEscape,
    UnsupportedCommandSubstitution,
    UnsupportedHereDocument,
    UnsupportedRedirection,
    MissingCommandAfterOperator,
}

impl std::fmt::Display for ShellParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::EmptyCommand => "commande shell vide",
            Self::UnterminatedSingleQuote => "quote simple non terminée",
            Self::UnterminatedDoubleQuote => "quote double non terminée",
            Self::TrailingEscape => "échappement final non terminé",
            Self::UnsupportedCommandSubstitution => "substitution de commande non autorisée",
            Self::UnsupportedHereDocument => "here-document non autorisé",
            Self::UnsupportedRedirection => "redirection shell non autorisée",
            Self::MissingCommandAfterOperator => "commande manquante après un opérateur shell",
        };
        f.write_str(message)
    }
}

impl std::error::Error for ShellParseError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quote {
    Single,
    Double,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LexItem {
    Word(String),
    Operator(ShellOperator),
}

pub fn parse_shell(command: &str) -> Result<ParsedShellCommand, ShellParseError> {
    let items = lex(command)?;
    let mut segments = Vec::new();
    let mut operators = Vec::new();
    let mut words = Vec::new();

    for item in items {
        match item {
            LexItem::Word(word) => words.push(word),
            LexItem::Operator(operator) => {
                if words.is_empty() {
                    return Err(ShellParseError::MissingCommandAfterOperator);
                }
                segments.push(make_segment(&mut words));
                operators.push(operator);
            }
        }
    }

    if words.is_empty() {
        return Err(ShellParseError::MissingCommandAfterOperator);
    }
    segments.push(make_segment(&mut words));

    Ok(ParsedShellCommand {
        segments,
        operators,
        has_environment_expansion: command.chars().any(|ch| ch == '$'),
    })
}

fn make_segment(words: &mut Vec<String>) -> ShellSegment {
    let program = words.remove(0);
    ShellSegment {
        program,
        args: std::mem::take(words),
    }
}

fn lex(command: &str) -> Result<Vec<LexItem>, ShellParseError> {
    let chars: Vec<char> = command.chars().collect();
    let mut output = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut at_token_start = true;
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];

        if escaped {
            token.push(ch);
            escaped = false;
            at_token_start = false;
            index += 1;
            continue;
        }

        match quote {
            Some(Quote::Single) => {
                if ch == '\'' {
                    quote = None;
                } else {
                    token.push(ch);
                }
                index += 1;
                continue;
            }
            Some(Quote::Double) => {
                if ch == '"' {
                    quote = None;
                } else if ch == '\\' {
                    escaped = true;
                } else {
                    token.push(ch);
                }
                index += 1;
                continue;
            }
            None => {}
        }

        match ch {
            '\\' => escaped = true,
            '\'' => quote = Some(Quote::Single),
            '"' => quote = Some(Quote::Double),
            '$' if chars.get(index + 1) == Some(&'(') => {
                return Err(ShellParseError::UnsupportedCommandSubstitution);
            }
            '`' => return Err(ShellParseError::UnsupportedCommandSubstitution),
            '#' if at_token_start => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
                continue;
            }
            ' ' | '\t' | '\r' => {
                flush(&mut token, &mut output);
                at_token_start = true;
            }
            '\n' | ';' => {
                flush(&mut token, &mut output);
                push_operator(&mut output, ShellOperator::Sequence)?;
                at_token_start = true;
            }
            '|' => {
                flush(&mut token, &mut output);
                if chars.get(index + 1) == Some(&'|') {
                    push_operator(&mut output, ShellOperator::Or)?;
                    index += 1;
                } else {
                    push_operator(&mut output, ShellOperator::Pipe)?;
                }
                at_token_start = true;
            }
            '&' => {
                flush(&mut token, &mut output);
                if chars.get(index + 1) == Some(&'&') {
                    push_operator(&mut output, ShellOperator::And)?;
                    index += 1;
                } else {
                    push_operator(&mut output, ShellOperator::Background)?;
                }
                at_token_start = true;
            }
            '>' | '<' => {
                if ch == '<' && chars.get(index + 1) == Some(&'<') {
                    return Err(ShellParseError::UnsupportedHereDocument);
                }
                return Err(ShellParseError::UnsupportedRedirection);
            }
            _ => {
                token.push(ch);
                at_token_start = false;
            }
        }

        index += 1;
    }

    if escaped {
        return Err(ShellParseError::TrailingEscape);
    }
    match quote {
        Some(Quote::Single) => return Err(ShellParseError::UnterminatedSingleQuote),
        Some(Quote::Double) => return Err(ShellParseError::UnterminatedDoubleQuote),
        None => {}
    }

    flush(&mut token, &mut output);
    if !output.iter().any(|item| matches!(item, LexItem::Word(_))) {
        return Err(ShellParseError::EmptyCommand);
    }
    Ok(output)
}

fn flush(token: &mut String, output: &mut Vec<LexItem>) {
    if !token.is_empty() {
        output.push(LexItem::Word(std::mem::take(token)));
    }
}

fn push_operator(output: &mut Vec<LexItem>, operator: ShellOperator) -> Result<(), ShellParseError> {
    if !matches!(output.last(), Some(LexItem::Word(_))) {
        return Err(ShellParseError::MissingCommandAfterOperator);
    }
    output.push(LexItem::Operator(operator));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_quotes_and_pipe() {
        let parsed = parse_shell("cat 'a file' | grep \"foo\"").unwrap();
        assert_eq!(parsed.segments[0].program, "cat");
        assert_eq!(parsed.segments[0].args, vec!["a file"]);
        assert_eq!(parsed.segments[1].program, "grep");
        assert!(!parsed.has_environment_expansion);
        assert_eq!(parsed.normalized(), "cat a file | grep foo");
    }

    #[test]
    fn reject_dynamic_substitution() {
        assert_eq!(
            parse_shell("echo $(id)"),
            Err(ShellParseError::UnsupportedCommandSubstitution)
        );
        assert_eq!(
            parse_shell("echo `id`"),
            Err(ShellParseError::UnsupportedCommandSubstitution)
        );
    }

    #[test]
    fn classify_shell_operators() {
        let parsed = parse_shell("git status && git diff").unwrap();
        assert!(matches!(parsed.operators, [ShellOperator::And]));
        assert!(parsed.has_non_pipe_operator());
    }

    #[test]
    fn preserve_multiple_lines_as_sequence() {
        let parsed = parse_shell("echo one\ngit status").unwrap();
        assert!(matches!(parsed.operators, [ShellOperator::Sequence]));
    }

    #[test]
    fn reject_redirection_and_here_doc() {
        assert_eq!(
            parse_shell("echo hi > file"),
            Err(ShellParseError::UnsupportedRedirection)
        );
        assert_eq!(
            parse_shell("cat <<EOF"),
            Err(ShellParseError::UnsupportedHereDocument)
        );
    }
}
