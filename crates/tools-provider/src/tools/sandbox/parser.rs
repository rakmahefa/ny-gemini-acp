//! Lexical parsing and normalization for shell sandbox decisions.
//!
//! This is intentionally not a full shell interpreter. It recognizes the shell
//! constructs that affect the safety boundary and produces a deterministic
//! representation for policy evaluation.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellOperator {
    Pipe,
    Sequence,
    And,
    Or,
    Background,
    Redirect,
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
                let operator = self.operators[index - 1];
                result.push_str(match operator {
                    ShellOperator::Pipe => " | ",
                    ShellOperator::Sequence => " ; ",
                    ShellOperator::And => " && ",
                    ShellOperator::Or => " || ",
                    ShellOperator::Background => " & ",
                    ShellOperator::Redirect => " > ",
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

fn flush_token(token: &mut String, tokens: &mut Vec<String>) {
    if !token.is_empty() {
        tokens.push(std::mem::take(token));
    }
}

pub fn parse_shell(command: &str) -> Result<ParsedShellCommand, ShellParseError> {
    let mut tokens = Vec::<String>::new();
    let mut operators = Vec::<ShellOperator>::new();
    let mut token = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut at_token_start = true;
    let mut has_environment_expansion = false;
    let chars: Vec<char> = command.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        let current = chars[index];

        if escaped {
            token.push(current);
            escaped = false;
            at_token_start = false;
            index += 1;
            continue;
        }

        match quote {
            Some(Quote::Single) => {
                if current == '\'' {
                    quote = None;
                } else {
                    token.push(current);
                }
                index += 1;
                continue;
            }
            Some(Quote::Double) => {
                if current == '"' {
                    quote = None;
                } else if current == '\\' {
                    escaped = true;
                } else {
                    if current == '$' {
                        has_environment_expansion = true;
                    }
                    token.push(current);
                }
                index += 1;
                continue;
            }
            None => {}
        }

        match current {
            '\\' => escaped = true,
            '\'' => quote = Some(Quote::Single),
            '"' => quote = Some(Quote::Double),
            '$' => {
                if chars.get(index + 1) == Some(&'(') {
                    return Err(ShellParseError::UnsupportedCommandSubstitution);
                }
                has_environment_expansion = true;
                token.push(current);
                at_token_start = false;
            }
            '`' => return Err(ShellParseError::UnsupportedCommandSubstitution),
            '#' if at_token_start => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
                continue;
            }
            ' ' | '\t' | '\r' => {
                flush_token(&mut token, &mut tokens);
                at_token_start = true;
            }
            '\n' => {
                flush_token(&mut token, &mut tokens);
                if !tokens.is_empty() {
                    operators.push(ShellOperator::Sequence);
                }
                at_token_start = true;
            }
            '|' => {
                flush_token(&mut token, &mut tokens);
                if chars.get(index + 1) == Some(&'|') {
                    operators.push(ShellOperator::Or);
                    index += 1;
                } else {
                    operators.push(ShellOperator::Pipe);
                }
                at_token_start = true;
            }
            '&' => {
                flush_token(&mut token, &mut tokens);
                if chars.get(index + 1) == Some(&'&') {
                    operators.push(ShellOperator::And);
                    index += 1;
                } else {
                    operators.push(ShellOperator::Background);
                }
                at_token_start = true;
            }
            ';' => {
                flush_token(&mut token, &mut tokens);
                operators.push(ShellOperator::Sequence);
                at_token_start = true;
            }
            '>' | '<' => {
                flush_token(&mut token, &mut tokens);
                if current == '<' && chars.get(index + 1) == Some(&'<') {
                    return Err(ShellParseError::UnsupportedHereDocument);
                }
                if current == '>' && chars.get(index + 1) == Some(&'>') {
                    index += 1;
                }
                operators.push(ShellOperator::Redirect);
                at_token_start = true;
            }
            _ => {
                token.push(current);
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

    flush_token(&mut token, &mut tokens);
    if tokens.is_empty() {
        return Err(ShellParseError::EmptyCommand);
    }

    let mut segments = Vec::new();
    let mut segment_tokens = Vec::new();
    for (index, value) in tokens.into_iter().enumerate() {
        segment_tokens.push(value);
        let boundary = index + 1 == segment_tokens.len();
        if boundary && index + 1 < 0 {
            unreachable!();
        }
    }

    // Re-tokenize while associating operators with complete command segments.
    // The operator count is derived from the lexical stream and therefore cannot
    // accidentally disagree with the segment count.
    let mut raw_segments = Vec::<Vec<String>>::new();
    let mut current_segment = Vec::<String>::new();
    let mut op_iter = operators.iter();
    let mut token_iter = command_tokens_without_operators(command)?.into_iter();
    let mut current_operator = None;
    while let Some(value) = token_iter.next() {
        current_segment.push(value);
        if current_segment.len() == 1 {
            // The next operator is determined by the normalized scanner below.
            let _ = &mut current_operator;
        }
    }
    let _ = op_iter.next();
    let _ = &mut current_operator;

    // A small second pass is simpler and safer than trying to reconstruct shell
    // grammar from whitespace. It uses the same lexical rules but keeps segment
    // boundaries explicit.
    let (final_segments, final_operators, expansion) = lex_segments(command)?;
    segments = final_segments;
    operators = final_operators;
    has_environment_expansion |= expansion;

    if segments.is_empty() || operators.len() + 1 < segments.len() {
        return Err(ShellParseError::MissingCommandAfterOperator);
    }
    if operators.len() + 1 != segments.len() {
        return Err(ShellParseError::MissingCommandAfterOperator);
    }

    Ok(ParsedShellCommand {
        segments,
        operators,
        has_environment_expansion,
    })
}

fn command_tokens_without_operators(command: &str) -> Result<Vec<String>, ShellParseError> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let chars: Vec<char> = command.chars().collect();
    let mut escaped = false;
    for current_char in chars {
        if escaped {
            current.push(current_char);
            escaped = false;
            continue;
        }
        match quote {
            Some(Quote::Single) if current_char == '\'' => quote = None,
            Some(Quote::Single) => current.push(current_char),
            Some(Quote::Double) if current_char == '"' => quote = None,
            Some(Quote::Double) => current.push(current_char),
            None => match current_char {
                '\\' => escaped = true,
                '\'' => quote = Some(Quote::Single),
                '"' => quote = Some(Quote::Double),
                ' ' | '\t' | '\r' | '\n' => flush_token(&mut current, &mut values),
                '|' | '&' | ';' | '>' | '<' => flush_token(&mut current, &mut values),
                _ => current.push(current_char),
            },
        }
    }
    flush_token(&mut current, &mut values);
    Ok(values)
}

fn lex_segments(
    command: &str,
) -> Result<(Vec<ShellSegment>, Vec<ShellOperator>, bool), ShellParseError> {
    let mut tokens = Vec::<String>::new();
    let mut lexical_ops = Vec::<Option<ShellOperator>>::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut has_environment_expansion = false;
    let chars: Vec<char> = command.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            current.push(ch);
            escaped = false;
            index += 1;
            continue;
        }
        match quote {
            Some(Quote::Single) => {
                if ch == '\'' { quote = None; } else { current.push(ch); }
                index += 1;
                continue;
            }
            Some(Quote::Double) => {
                if ch == '"' { quote = None; }
                else if ch == '\\' { escaped = true; }
                else { if ch == '$' { has_environment_expansion = true; } current.push(ch); }
                index += 1;
                continue;
            }
            None => {}
        }
        match ch {
            '\\' => escaped = true,
            '\'' => quote = Some(Quote::Single),
            '"' => quote = Some(Quote::Double),
            '$' => {
                if chars.get(index + 1) == Some(&'(') { return Err(ShellParseError::UnsupportedCommandSubstitution); }
                has_environment_expansion = true;
                current.push(ch);
            }
            '`' => return Err(ShellParseError::UnsupportedCommandSubstitution),
            '#' if current.is_empty() => {
                while index < chars.len() && chars[index] != '\n' { index += 1; }
                continue;
            }
            ' ' | '\t' | '\r' | '\n' => flush_token(&mut current, &mut tokens),
            '|' | '&' | ';' | '>' | '<' => {
                flush_token(&mut current, &mut tokens);
                let op = match ch {
                    '|' if chars.get(index + 1) == Some(&'|') => { index += 1; ShellOperator::Or }
                    '|' => ShellOperator::Pipe,
                    '&' if chars.get(index + 1) == Some(&'&') => { index += 1; ShellOperator::And }
                    '&' => ShellOperator::Background,
                    ';' => ShellOperator::Sequence,
                    '>' => {
                        if chars.get(index + 1) == Some(&'>') { index += 1; }
                        ShellOperator::Redirect
                    }
                    '<' => {
                        if chars.get(index + 1) == Some(&'<') { return Err(ShellParseError::UnsupportedHereDocument); }
                        ShellOperator::Redirect
                    }
                    _ => unreachable!(),
                };
                lexical_ops.push(Some(op));
            }
            _ => {
                current.push(ch);
                lexical_ops.push(None);
            }
        }
        index += 1;
    }
    if escaped { return Err(ShellParseError::TrailingEscape); }
    match quote {
        Some(Quote::Single) => return Err(ShellParseError::UnterminatedSingleQuote),
        Some(Quote::Double) => return Err(ShellParseError::UnterminatedDoubleQuote),
        None => {}
    }
    flush_token(&mut current, &mut tokens);
    if tokens.is_empty() { return Err(ShellParseError::EmptyCommand); }

    // Parse the command again with a focused scanner that records operator
    // boundaries by token count. This avoids making policy decisions on raw text.
    let mut segments = Vec::<ShellSegment>::new();
    let mut operators = Vec::<ShellOperator>::new();
    let mut segment = Vec::<String>::new();
    let mut pending_operator = None;
    let mut token_index = 0usize;
    let _ = lexical_ops;
    let parsed_tokens = command_tokens_with_ops(command)?;
    for item in parsed_tokens {
        match item {
            LexItem::Word(value) => {
                segment.push(value);
                token_index += 1;
            }
            LexItem::Operator(operator) => {
                if segment.is_empty() { return Err(ShellParseError::MissingCommandAfterOperator); }
                let program = segment.remove(0);
                segments.push(ShellSegment { program, args: segment });
                segment = Vec::new();
                pending_operator = Some(operator);
                if let Some(op) = pending_operator.take() { operators.push(op); }
            }
        }
    }
    if segment.is_empty() { return Err(ShellParseError::MissingCommandAfterOperator); }
    let program = segment.remove(0);
    segments.push(ShellSegment { program, args: segment });
    let _ = token_index;
    Ok((segments, operators, has_environment_expansion))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LexItem {
    Word(String),
    Operator(ShellOperator),
}

fn command_tokens_with_ops(command: &str) -> Result<Vec<LexItem>, ShellParseError> {
    let mut output = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let chars: Vec<char> = command.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if escaped { current.push(ch); escaped = false; index += 1; continue; }
        match quote {
            Some(Quote::Single) => {
                if ch == '\'' { quote = None; } else { current.push(ch); }
                index += 1; continue;
            }
            Some(Quote::Double) => {
                if ch == '"' { quote = None; } else if ch == '\\' { escaped = true; } else { current.push(ch); }
                index += 1; continue;
            }
            None => {}
        }
        match ch {
            '\\' => escaped = true,
            '\'' => quote = Some(Quote::Single),
            '"' => quote = Some(Quote::Double),
            '$' if chars.get(index + 1) == Some(&'(') => return Err(ShellParseError::UnsupportedCommandSubstitution),
            '`' => return Err(ShellParseError::UnsupportedCommandSubstitution),
            ' ' | '\t' | '\r' | '\n' => flush_token_item(&mut current, &mut output),
            '#' if current.is_empty() => {
                while index < chars.len() && chars[index] != '\n' { index += 1; }
                continue;
            }
            '|' | '&' | ';' | '>' | '<' => {
                flush_token_item(&mut current, &mut output);
                let op = match ch {
                    '|' if chars.get(index + 1) == Some(&'|') => { index += 1; ShellOperator::Or }
                    '|' => ShellOperator::Pipe,
                    '&' if chars.get(index + 1) == Some(&'&') => { index += 1; ShellOperator::And }
                    '&' => ShellOperator::Background,
                    ';' => ShellOperator::Sequence,
                    '>' => { if chars.get(index + 1) == Some(&'>') { index += 1; } ShellOperator::Redirect }
                    '<' => { if chars.get(index + 1) == Some(&'<') { return Err(ShellParseError::UnsupportedHereDocument); } ShellOperator::Redirect }
                    _ => unreachable!(),
                };
                output.push(LexItem::Operator(op));
            }
            _ => current.push(ch),
        }
        index += 1;
    }
    if escaped { return Err(ShellParseError::TrailingEscape); }
    match quote {
        Some(Quote::Single) => return Err(ShellParseError::UnterminatedSingleQuote),
        Some(Quote::Double) => return Err(ShellParseError::UnterminatedDoubleQuote),
        None => {}
    }
    flush_token_item(&mut current, &mut output);
    Ok(output)
}

fn flush_token_item(token: &mut String, output: &mut Vec<LexItem>) {
    if !token.is_empty() { output.push(LexItem::Word(std::mem::take(token))); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_quotes_and_pipe() {
        let parsed = parse_shell("cat 'a file' | grep \\\"foo\\\"").unwrap();
        assert_eq!(parsed.segments[0].program, "cat");
        assert_eq!(parsed.segments[0].args, vec!["a file"]);
        assert_eq!(parsed.segments[1].program, "grep");
        assert!(parsed.has_environment_expansion == false);
        assert_eq!(parsed.normalized(), "cat a file | grep foo");
    }

    #[test]
    fn reject_dynamic_substitution() {
        assert_eq!(parse_shell("echo $(id)"), Err(ShellParseError::UnsupportedCommandSubstitution));
        assert_eq!(parse_shell("echo `id`"), Err(ShellParseError::UnsupportedCommandSubstitution));
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
}
