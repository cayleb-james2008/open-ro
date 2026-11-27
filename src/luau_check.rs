//! Static Luau Analyzer & Cross-Reference Prover for OpenRO.
//!
//! Performs real lexical, syntactic, control-flow, security, and cross-script
//! Place verification on Luau source files with zero external runtime required:
//!
//! 1. **Syntax & Block Balance**: string literals (`"..."`, `'...'`, `` `...` ``,
//!    `[[...]]`, `[=[...]=]`), block comments (`--[[...]]`, `--[=[...]=]`),
//!    delimiter balance (`()`, `[]`, `{}`), and Luau keyword block balance
//!    (`function`/`if`/`for`/`while`/`repeat`/`do` vs `end`/`until`), including
//!    distinguishing Luau `if ... then ... else ...` expressions from statements.
//! 2. **Infinite Loop Detection**: catches `while true do` / `while 1 do` loops
//!    that lack a yielding call (`task.wait`, `wait`, `:Wait`) or loop exit
//!    (`break`/`return`) at the loop's own function depth.
//! 3. **DataStore Safety**: catches unguarded `DataStoreService` async calls
//!    (`GetAsync`/`SetAsync`/`UpdateAsync`/`IncrementAsync`/`RemoveAsync`/
//!    `GetSortedAsync`) outside `pcall`/`xpcall` (or local `pcall`-wrapping
//!    helpers), plus `GetDataStore` scripts missing `BindToClose`.
//! 4. **RemoteEvent Security**: catches client-trusting `OnServerEvent` /
//!    `OnServerInvoke` handlers that accept client parameters beyond `player`
//!    without validating them (`typeof`/`type`/`:IsA`/`tonumber`/`tostring`/
//!    `math.clamp` or `if`-guard checks).
//! 5. **Cross-Reference Place Prover**: verifies `workspace.<Member>`,
//!    `<Service>:WaitForChild("Name")`, and `<Service>:FindFirstChild("Name")`
//!    against actual Instances, Scripts, and dynamically created Instances
//!    across the Place.

use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

pub const VALID_SERVICES: [&str; 9] = [
    "Workspace",
    "ServerScriptService",
    "StarterGui",
    "StarterPlayer",
    "ReplicatedStorage",
    "ServerStorage",
    "Lighting",
    "SoundService",
    "StarterPack",
];

const WORKSPACE_BUILTINS: [&str; 19] = [
    "CurrentCamera",
    "Gravity",
    "Terrain",
    "DistributedGameTime",
    "FallenPartsDestroyHeight",
    "StreamingEnabled",
    "GetServerTimeNow",
    "FindFirstChild",
    "FindFirstChildOfClass",
    "FindFirstChildWhichIsA",
    "WaitForChild",
    "GetChildren",
    "GetDescendants",
    "ChildAdded",
    "ChildRemoved",
    "Raycast",
    "Name",
    "Parent",
    "ClassName",
];

const DATASTORE_ASYNC_METHODS: [&str; 6] = [
    "GetAsync",
    "SetAsync",
    "UpdateAsync",
    "IncrementAsync",
    "RemoveAsync",
    "GetSortedAsync",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub path: String,
    pub line: usize,
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
}

impl Diagnostic {
    pub fn to_json(&self) -> Value {
        json!({
            "path": self.path,
            "line": self.line,
            "severity": self.severity.as_str(),
            "code": self.code,
            "message": self.message,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct AnalysisReport {
    pub diagnostics: Vec<Diagnostic>,
}

impl AnalysisReport {
    pub fn is_ok(&self) -> bool {
        !self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    pub fn errors(&self) -> Vec<String> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message.clone())
            .collect()
    }

    pub fn warnings(&self) -> Vec<String> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .map(|d| d.message.clone())
            .collect()
    }

    pub fn has_code(&self, code: &str) -> bool {
        self.diagnostics.iter().any(|d| d.code == code)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "ok": self.is_ok(),
            "error_count": self.error_count(),
            "warning_count": self.warning_count(),
            "diagnostics": self.diagnostics.iter().map(Diagnostic::to_json).collect::<Vec<_>>(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TokenKind {
    Ident(String),
    StringLit(String),
    Number(String),
    Punct(String),
}

#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    line: usize,
    col: usize,
    /// True when immediately preceded by single `.` or `:` (member/method position).
    is_member: bool,
}

impl Token {
    fn span_len(&self) -> usize {
        match &self.kind {
            TokenKind::Ident(s) | TokenKind::Number(s) | TokenKind::Punct(s) => s.chars().count(),
            TokenKind::StringLit(s) => s.chars().count() + 2,
        }
    }

    fn is_kw(&self, kw: &str) -> bool {
        !self.is_member && matches!(&self.kind, TokenKind::Ident(s) if s == kw)
    }

    fn is_punct(&self, p: &str) -> bool {
        matches!(&self.kind, TokenKind::Punct(s) if s == p)
    }

    fn ident(&self) -> Option<&str> {
        match &self.kind {
            TokenKind::Ident(s) => Some(s.as_str()),
            _ => None,
        }
    }

    fn str_lit(&self) -> Option<&str> {
        match &self.kind {
            TokenKind::StringLit(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

/// Match `[=*[` at `chars[i]`, returning `(eq_count, content_start_index)` if matched.
fn match_long_bracket_open(chars: &[char], i: usize) -> Option<(usize, usize)> {
    if i >= chars.len() || chars[i] != '[' {
        return None;
    }
    let mut j = i + 1;
    while j < chars.len() && chars[j] == '=' {
        j += 1;
    }
    if j < chars.len() && chars[j] == '[' {
        Some((j - i - 1, j + 1))
    } else {
        None
    }
}

/// Scan until matching `]=*]` with `eq_count` equals signs. Returns `Some((content, next_idx))` or `None` if unclosed.
fn scan_long_bracket_close(
    chars: &[char],
    start: usize,
    eq_count: usize,
    line: &mut usize,
    line_start: &mut usize,
) -> Option<(String, usize)> {
    let mut i = start;
    let mut content = String::new();
    while i < chars.len() {
        if chars[i] == ']' {
            let mut j = i + 1;
            let mut eqs = 0;
            while j < chars.len() && chars[j] == '=' && eqs < eq_count {
                j += 1;
                eqs += 1;
            }
            if eqs == eq_count && j < chars.len() && chars[j] == ']' {
                return Some((content, j + 1));
            }
        }
        if chars[i] == '\n' {
            *line += 1;
            *line_start = i + 1;
        }
        content.push(chars[i]);
        i += 1;
    }
    None
}

const MAX_LUAU_ANALYSIS_BYTES: usize = 2 * 1024 * 1024;
const MAX_LUAU_TOKENS: usize = 250_000;
const MAX_EXPR_DEPTH: usize = 64;

struct LexError {
    line: usize,
    code: &'static str,
    message: String,
}

fn tokenize(source: &str) -> Result<Vec<Token>, LexError> {
    let source = source.strip_prefix('\u{FEFF}').unwrap_or(source);
    if source.len() > MAX_LUAU_ANALYSIS_BYTES {
        return Err(LexError {
            line: 1,
            code: "FILE_TOO_LARGE",
            message: format!("1: Luau file exceeds {MAX_LUAU_ANALYSIS_BYTES} byte analysis limit"),
        });
    }
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::with_capacity((source.len() / 5 + 8).min(65_536));
    let mut delims: Vec<(char, usize)> = Vec::new();
    let mut line: usize = 1;
    let mut line_start: usize = 0;
    let mut i: usize = 0;

    while i < chars.len() {
        if tokens.len() >= MAX_LUAU_TOKENS {
            return Err(LexError {
                line,
                code: "TOO_MANY_TOKENS",
                message: format!("{line}: Luau file exceeds {MAX_LUAU_TOKENS} token limit"),
            });
        }
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
            line_start = i;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // Comments: `--` or `--[=*[...]=*]`
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            if let Some((eqs, content_start)) = match_long_bracket_open(&chars, i + 2) {
                let start_line = line;
                match scan_long_bracket_close(&chars, content_start, eqs, &mut line, &mut line_start) {
                    Some((_, next)) => {
                        i = next;
                    }
                    None => {
                        return Err(LexError {
                            line: start_line,
                            code: "UNCLOSED_COMMENT",
                            message: format!("{start_line}: unclosed block comment --[["),
                        });
                    }
                }
            } else {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            continue;
        }

        // Quoted or interpolated strings: `"..."`, `'...'`, `` `...` ``
        if c == '"' || c == '\'' || c == '`' {
            let start_line = line;
            let start_col = i - line_start + 1;
            let q = c;
            i += 1;
            let mut val = String::new();
            let mut closed = false;
            while i < chars.len() {
                let ch = chars[i];
                if ch == '\\' && i + 1 < chars.len() {
                    let next = chars[i + 1];
                    match next {
                        'n' => val.push('\n'),
                        'r' => val.push('\r'),
                        't' => val.push('\t'),
                        '\\' => val.push('\\'),
                        '"' => val.push('"'),
                        '\'' => val.push('\''),
                        '`' => val.push('`'),
                        '\n' => {
                            line += 1;
                            line_start = i + 2;
                            val.push('\n');
                        }
                        other => val.push(other),
                    }
                    i += 2;
                    continue;
                }
                if ch == '\n' && q != '`' {
                    return Err(LexError {
                        line: start_line,
                        code: "UNCLOSED_STRING",
                        message: format!("{start_line}: unclosed string literal"),
                    });
                }
                if ch == '\n' {
                    line += 1;
                    line_start = i + 1;
                }
                if ch == q {
                    i += 1;
                    closed = true;
                    break;
                }
                val.push(ch);
                i += 1;
            }
            if !closed {
                return Err(LexError {
                    line: start_line,
                    code: "UNCLOSED_STRING",
                    message: format!("{start_line}: unclosed string literal"),
                });
            }
            tokens.push(Token {
                kind: TokenKind::StringLit(val),
                line: start_line,
                col: start_col,
                is_member: false,
            });
            continue;
        }

        // Long string `[=*[...]=*]`
        if let Some((eqs, content_start)) = match_long_bracket_open(&chars, i) {
            let start_line = line;
            let start_col = i - line_start + 1;
            match scan_long_bracket_close(&chars, content_start, eqs, &mut line, &mut line_start) {
                Some((content, next)) => {
                    tokens.push(Token {
                        kind: TokenKind::StringLit(content),
                        line: start_line,
                        col: start_col,
                        is_member: false,
                    });
                    i = next;
                    continue;
                }
                None => {
                    return Err(LexError {
                        line: start_line,
                        code: "UNCLOSED_STRING",
                        message: format!("{start_line}: unclosed long string [["),
                    });
                }
            }
        }

        // Numbers
        if c.is_ascii_digit() {
            let start = i;
            let start_col = start - line_start + 1;
            if c == '0' && i + 1 < chars.len() && matches!(chars[i + 1], 'x' | 'X' | 'b' | 'B') {
                i += 2;
                while i < chars.len() && (chars[i].is_ascii_hexdigit() || chars[i] == '_') {
                    i += 1;
                }
            } else {
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                    i += 1;
                }
                if i < chars.len()
                    && chars[i] == '.'
                    && (i + 1 >= chars.len() || chars[i + 1] != '.')
                {
                    i += 1;
                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                        i += 1;
                    }
                }
                if i < chars.len() && matches!(chars[i], 'e' | 'E') {
                    i += 1;
                    if i < chars.len() && matches!(chars[i], '+' | '-') {
                        i += 1;
                    }
                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                        i += 1;
                    }
                }
            }
            let num: String = chars[start..i].iter().collect();
            tokens.push(Token {
                kind: TokenKind::Number(num),
                line,
                col: start_col,
                is_member: false,
            });
            continue;
        }

        // Identifiers & keywords
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            let start_col = start - line_start + 1;
            let prev_is_member = matches!(
                tokens.last().map(|t| &t.kind),
                Some(TokenKind::Punct(p)) if p == "." || p == ":"
            );
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            tokens.push(Token {
                kind: TokenKind::Ident(word),
                line,
                col: start_col,
                is_member: prev_is_member,
            });
            continue;
        }

        // Delimiters
        if matches!(c, '(' | '[' | '{') {
            let start_col = i - line_start + 1;
            delims.push((c, line));
            tokens.push(Token {
                kind: TokenKind::Punct(c.to_string()),
                line,
                col: start_col,
                is_member: false,
            });
            i += 1;
            continue;
        }
        if matches!(c, ')' | ']' | '}') {
            let start_col = i - line_start + 1;
            let expected = match c {
                ')' => '(',
                ']' => '[',
                '}' => '{',
                _ => unreachable!(),
            };
            match delims.pop() {
                Some((open, _)) if open == expected => {}
                Some((open, open_line)) => {
                    return Err(LexError {
                        line,
                        code: "MISMATCHED_DELIM",
                        message: format!(
                            "{line}: mismatched delimiter '{c}' (expected closing for '{open}' from line {open_line})"
                        ),
                    });
                }
                None => {
                    return Err(LexError {
                        line,
                        code: "MISMATCHED_DELIM",
                        message: format!("{line}: unexpected closing delimiter '{c}'"),
                    });
                }
            }
            tokens.push(Token {
                kind: TokenKind::Punct(c.to_string()),
                line,
                col: start_col,
                is_member: false,
            });
            i += 1;
            continue;
        }

        // Multi-character operators
        if c == '.' && i + 2 < chars.len() && chars[i + 1] == '.' && chars[i + 2] == '=' {
            let start_col = i - line_start + 1;
            tokens.push(Token {
                kind: TokenKind::Punct("..=".to_string()),
                line,
                col: start_col,
                is_member: false,
            });
            i += 3;
            continue;
        }
        if c == '.' && i + 2 < chars.len() && chars[i + 1] == '.' && chars[i + 2] == '.' {
            let start_col = i - line_start + 1;
            tokens.push(Token {
                kind: TokenKind::Punct("...".to_string()),
                line,
                col: start_col,
                is_member: false,
            });
            i += 3;
            continue;
        }
        if i + 1 < chars.len() {
            let two = (c, chars[i + 1]);
            let matched = match two {
                ('.', '.') => Some(".."),
                ('=', '=') => Some("=="),
                ('~', '=') => Some("~="),
                ('<', '=') => Some("<="),
                ('>', '=') => Some(">="),
                ('+', '=') => Some("+="),
                ('-', '=') => Some("-="),
                ('*', '=') => Some("*="),
                ('/', '=') => Some("/="),
                ('%', '=') => Some("%="),
                ('^', '=') => Some("^="),
                (':', ':') => Some("::"),
                ('-', '>') => Some("->"),
                _ => None,
            };
            if let Some(op) = matched {
                let start_col = i - line_start + 1;
                tokens.push(Token {
                    kind: TokenKind::Punct(op.to_string()),
                    line,
                    col: start_col,
                    is_member: false,
                });
                i += 2;
                continue;
            }
        }

        // Single-character punctuation
        let start_col = i - line_start + 1;
        tokens.push(Token {
            kind: TokenKind::Punct(c.to_string()),
            line,
            col: start_col,
            is_member: false,
        });
        i += 1;
    }

    if let Some((open, open_line)) = delims.last() {
        return Err(LexError {
            line: *open_line,
            code: "UNCLOSED_DELIM",
            message: format!("{open_line}: unclosed delimiter '{open}'"),
        });
    }

    Ok(tokens)
}

/// Returns true when the token immediately preceding `if` indicates a Luau
/// `if ... then ... else ...` expression (which does not take a closing `end`).
fn is_if_expression_context(prev: Option<&Token>) -> bool {
    let Some(t) = prev else { return false };
    match &t.kind {
        TokenKind::Punct(p) => matches!(
            p.as_str(),
            "=" | "("
                | ","
                | "{"
                | "+"
                | "-"
                | "*"
                | "/"
                | "%"
                | "^"
                | ".."
                | "=="
                | "~="
                | "<"
                | "<="
                | ">"
                | ">="
                | "+="
                | "-="
                | "*="
                | "/="
                | "..="
        ),
        TokenKind::Ident(w) if !t.is_member => {
            matches!(w.as_str(), "return" | "and" | "or" | "not")
        }
        _ => false,
    }
}

#[derive(Clone, Copy)]
struct OpenBlock {
    kind: &'static str,
    line: usize,
    awaiting_do: bool,
}

fn check_block_balance(tokens: &[Token]) -> Result<(), (usize, String)> {
    let mut blocks: Vec<OpenBlock> = Vec::new();

    for (idx, tok) in tokens.iter().enumerate() {
        if tok.is_member {
            continue;
        }
        let Some(word) = tok.ident() else { continue };
        match word {
            "function" => blocks.push(OpenBlock {
                kind: "function",
                line: tok.line,
                awaiting_do: false,
            }),
            "if" => {
                let prev = if idx > 0 { Some(&tokens[idx - 1]) } else { None };
                if !is_if_expression_context(prev) {
                    blocks.push(OpenBlock {
                        kind: "if",
                        line: tok.line,
                        awaiting_do: false,
                    });
                }
            }
            "for" => blocks.push(OpenBlock {
                kind: "for",
                line: tok.line,
                awaiting_do: true,
            }),
            "while" => blocks.push(OpenBlock {
                kind: "while",
                line: tok.line,
                awaiting_do: true,
            }),
            "do" => {
                if let Some(top) = blocks.last_mut() {
                    if top.awaiting_do {
                        top.awaiting_do = false;
                        continue;
                    }
                }
                blocks.push(OpenBlock {
                    kind: "do",
                    line: tok.line,
                    awaiting_do: false,
                });
            }
            "repeat" => blocks.push(OpenBlock {
                kind: "repeat",
                line: tok.line,
                awaiting_do: false,
            }),
            "until" => match blocks.pop() {
                Some(OpenBlock { kind: "repeat", .. }) => {}
                Some(other) => {
                    return Err((
                        tok.line,
                        format!(
                            "{}: unexpected 'until' (open '{}' from line {})",
                            tok.line, other.kind, other.line
                        ),
                    ));
                }
                None => {
                    return Err((
                        tok.line,
                        format!("{}: unexpected 'until' without 'repeat'", tok.line),
                    ));
                }
            },
            "end" => match blocks.pop() {
                Some(OpenBlock {
                    kind: "repeat",
                    line: l,
                    ..
                }) => {
                    return Err((
                        tok.line,
                        format!(
                            "{}: expected 'until' to close 'repeat' from line {l}, found 'end'",
                            tok.line
                        ),
                    ));
                }
                Some(_) => {}
                None => {
                    return Err((
                        tok.line,
                        format!("{}: unexpected 'end' with no matching block", tok.line),
                    ));
                }
            },
            _ => {}
        }
    }

    if let Some(open) = blocks.last() {
        let closer = if open.kind == "repeat" { "until" } else { "end" };
        return Err((
            open.line,
            format!(
                "{}: unclosed '{}' block (missing '{closer}')",
                open.line, open.kind
            ),
        ));
    }

    Ok(())
}

/// Real Luau syntax checker verifying string termination, delimiter balance,
/// and keyword block balance (`function`/`if`/`for`/`while`/`do`/`repeat` -> `end`/`until`).
pub fn check_luau_syntax(source: &str) -> Result<(), String> {
    if source.trim().is_empty() {
        return Err("1: empty Luau script".to_string());
    }
    let tokens = tokenize(source).map_err(|e| e.message)?;
    check_block_balance(&tokens).map_err(|(_, msg)| msg)
}

/// Given the index of a block opener (`while`/`for`/`function`/`do`/`if`), find the
/// index of its matching `end` token (assuming balanced tokens).
fn find_matching_end(tokens: &[Token], opener_idx: usize) -> Option<usize> {
    let mut blocks: Vec<OpenBlock> = Vec::new();
    let start_word = tokens.get(opener_idx)?.ident()?;
    let awaiting_do = matches!(start_word, "for" | "while");
    blocks.push(OpenBlock {
        kind: match start_word {
            "function" => "function",
            "if" => "if",
            "for" => "for",
            "while" => "while",
            _ => "do",
        },
        line: tokens[opener_idx].line,
        awaiting_do,
    });

    for idx in (opener_idx + 1)..tokens.len() {
        let tok = &tokens[idx];
        if tok.is_member {
            continue;
        }
        let Some(word) = tok.ident() else { continue };
        match word {
            "function" => blocks.push(OpenBlock {
                kind: "function",
                line: tok.line,
                awaiting_do: false,
            }),
            "if" => {
                if !is_if_expression_context(Some(&tokens[idx - 1])) {
                    blocks.push(OpenBlock {
                        kind: "if",
                        line: tok.line,
                        awaiting_do: false,
                    });
                }
            }
            "for" => blocks.push(OpenBlock {
                kind: "for",
                line: tok.line,
                awaiting_do: true,
            }),
            "while" => blocks.push(OpenBlock {
                kind: "while",
                line: tok.line,
                awaiting_do: true,
            }),
            "do" => {
                if let Some(top) = blocks.last_mut() {
                    if top.awaiting_do {
                        top.awaiting_do = false;
                        continue;
                    }
                }
                blocks.push(OpenBlock {
                    kind: "do",
                    line: tok.line,
                    awaiting_do: false,
                });
            }
            "repeat" => blocks.push(OpenBlock {
                kind: "repeat",
                line: tok.line,
                awaiting_do: false,
            }),
            "until" => {
                if matches!(blocks.last(), Some(OpenBlock { kind: "repeat", .. })) {
                    blocks.pop();
                }
            }
            "end" => {
                blocks.pop();
                if blocks.is_empty() {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

/// Detect `while true do` / `while 1 do` loops without `task.wait()`, `wait()`,
/// `:Wait()`, or `break`/`return` at the loop's own function depth.
fn check_infinite_loops(path: &str, tokens: &[Token], out: &mut Vec<Diagnostic>) {
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].is_kw("while") {
            let while_line = tokens[i].line;
            // Check whether condition is unconditionally truthy: `true`, `(true)`, `1`, `(1)`
            let mut j = i + 1;
            let mut paren_wrapped = false;
            if j < tokens.len() && tokens[j].is_punct("(") {
                paren_wrapped = true;
                j += 1;
            }
            let cond_is_true = if j < tokens.len() {
                tokens[j].is_kw("true")
                    || matches!(&tokens[j].kind, TokenKind::Number(n) if n == "1")
            } else {
                false
            };
            if cond_is_true {
                j += 1;
                if paren_wrapped && j < tokens.len() && tokens[j].is_punct(")") {
                    j += 1;
                }
                if j < tokens.len() && tokens[j].is_kw("do") {
                    if let Some(end_idx) = find_matching_end(tokens, i) {
                        if !loop_body_yields_or_exits(tokens, j + 1, end_idx) {
                            out.push(Diagnostic {
                                path: path.to_string(),
                                line: while_line,
                                severity: Severity::Error,
                                code: "INFINITE_LOOP_NO_YIELD",
                                message: format!(
                                    "{path}:{while_line}: infinite `while true do` loop has no `task.wait()`/`wait()` yield or exit and will freeze Roblox Studio"
                                ),
                            });
                        }
                    }
                }
            }
        }
        i += 1;
    }
}

fn loop_body_yields_or_exits(tokens: &[Token], body_start: usize, body_end: usize) -> bool {
    // Track nested function blocks inside the loop so a `task.wait()` or `return`
    // inside a nested callback (`task.spawn(function() ... end)`) does NOT excuse
    // an unyielding outer `while true do`.
    let mut inner_blocks: Vec<OpenBlock> = Vec::new();
    let mut func_depth: usize = 0;

    let mut k = body_start;
    while k < body_end {
        let tok = &tokens[k];
        if !tok.is_member {
            if let Some(word) = tok.ident() {
                match word {
                    "function" => {
                        inner_blocks.push(OpenBlock {
                            kind: "function",
                            line: tok.line,
                            awaiting_do: false,
                        });
                        func_depth += 1;
                        k += 1;
                        continue;
                    }
                    "if" => {
                        let prev = if k > 0 { Some(&tokens[k - 1]) } else { None };
                        if !is_if_expression_context(prev) {
                            inner_blocks.push(OpenBlock {
                                kind: "if",
                                line: tok.line,
                                awaiting_do: false,
                            });
                        }
                    }
                    "for" | "while" => {
                        inner_blocks.push(OpenBlock {
                            kind: if word == "for" { "for" } else { "while" },
                            line: tok.line,
                            awaiting_do: true,
                        });
                    }
                    "do" => {
                        if let Some(top) = inner_blocks.last_mut() {
                            if top.awaiting_do {
                                top.awaiting_do = false;
                                k += 1;
                                continue;
                            }
                        }
                        inner_blocks.push(OpenBlock {
                            kind: "do",
                            line: tok.line,
                            awaiting_do: false,
                        });
                    }
                    "repeat" => {
                        inner_blocks.push(OpenBlock {
                            kind: "repeat",
                            line: tok.line,
                            awaiting_do: false,
                        });
                    }
                    "until" => {
                        if matches!(inner_blocks.last(), Some(OpenBlock { kind: "repeat", .. })) {
                            inner_blocks.pop();
                        }
                    }
                    "end" => {
                        if let Some(popped) = inner_blocks.pop() {
                            if popped.kind == "function" && func_depth > 0 {
                                func_depth -= 1;
                            }
                        }
                    }
                    "break" | "return" if func_depth == 0 => return true,
                    "wait" if func_depth == 0 => {
                        if k + 1 < body_end && tokens[k + 1].is_punct("(") {
                            return true;
                        }
                    }
                    "task" if func_depth == 0 => {
                        if k + 2 < body_end
                            && tokens[k + 1].is_punct(".")
                            && tokens[k + 2].ident() == Some("wait")
                        {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
        } else if func_depth == 0 {
            // Method calls like `Heartbeat:Wait()` or `MoveToFinished:Wait()`
            if tok.ident() == Some("Wait")
                && k > body_start
                && tokens[k - 1].is_punct(":")
                && k + 1 < body_end
                && tokens[k + 1].is_punct("(")
            {
                return true;
            }
        }
        k += 1;
    }
    false
}

/// Find local helper function names whose body calls `pcall` or `xpcall` (e.g. `withRetry`).
fn find_pcall_wrappers(tokens: &[Token]) -> HashSet<String> {
    let mut wrappers = HashSet::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].is_kw("function") {
            if let Some(name_tok) = tokens.get(i + 1) {
                if !name_tok.is_member {
                    if let Some(name) = name_tok.ident() {
                        if tokens.get(i + 2).map(|t| t.is_punct("(")).unwrap_or(false) {
                            if let Some(end_idx) = find_matching_end(tokens, i) {
                                let uses_pcall = tokens[(i + 2)..end_idx].iter().any(|t| {
                                    t.is_kw("pcall") || t.is_kw("xpcall")
                                });
                                if uses_pcall {
                                    wrappers.insert(name.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }
    wrappers
}

/// Detect unguarded `DataStoreService` async calls missing `pcall`/`xpcall` and
/// `GetDataStore` scripts missing `BindToClose`.
fn check_datastore_safety(path: &str, tokens: &[Token], out: &mut Vec<Diagnostic>) {
    let wrappers = find_pcall_wrappers(tokens);
    let mut paren_depth: usize = 0;
    let mut protected_depths: Vec<usize> = Vec::new();
    let mut uses_datastore = false;
    let mut has_bind_to_close = false;
    let mut datastore_line: usize = 1;

    for (idx, tok) in tokens.iter().enumerate() {
        if tok.is_punct("(") {
            paren_depth += 1;
            // Check if the token right before `(` is `pcall`, `xpcall`, or a local pcall wrapper
            if idx > 0 {
                let prev = &tokens[idx - 1];
                if !prev.is_member {
                    if let Some(callee) = prev.ident() {
                        if callee == "pcall" || callee == "xpcall" || wrappers.contains(callee) {
                            protected_depths.push(paren_depth);
                        }
                    }
                }
            }
            continue;
        }
        if tok.is_punct(")") {
            while protected_depths.last() == Some(&paren_depth) {
                protected_depths.pop();
            }
            paren_depth = paren_depth.saturating_sub(1);
            continue;
        }

        if let Some(ident) = tok.ident() {
            if tok.is_member && (ident == "GetDataStore" || ident == "GetOrderedDataStore") {
                uses_datastore = true;
                datastore_line = tok.line;
            }
            if ident == "BindToClose" {
                has_bind_to_close = true;
            }
            if tok.is_member
                && DATASTORE_ASYNC_METHODS.contains(&ident)
                && protected_depths.is_empty()
            {
                out.push(Diagnostic {
                    path: path.to_string(),
                    line: tok.line,
                    severity: Severity::Error,
                    code: "UNGUARDED_DATASTORE_CALL",
                    message: format!(
                        "{path}:{}: unguarded DataStore call '{ident}' must be wrapped in pcall()",
                        tok.line
                    ),
                });
            }
        }
    }

    if uses_datastore && !has_bind_to_close {
        out.push(Diagnostic {
            path: path.to_string(),
            line: datastore_line,
            severity: Severity::Error,
            code: "MISSING_BIND_TO_CLOSE",
            message: format!("{path}: DataStore save failed: BindToClose was never bound"),
        });
    }
}

/// Detect `.OnServerEvent:Connect(function(player, ...))` and `.OnServerInvoke = function(player, ...)`
/// handlers that accept client arguments beyond `player` without validating them.
fn check_remote_validation(path: &str, tokens: &[Token], out: &mut Vec<Diagnostic>) {
    let mut i = 0;
    while i < tokens.len() {
        let mut func_idx: Option<usize> = None;
        // Pattern A: `.OnServerEvent:Connect(function(` or `:Once(function(`
        if tokens[i].is_member
            && tokens[i].ident() == Some("OnServerEvent")
            && tokens.get(i + 1).map(|t| t.is_punct(":")).unwrap_or(false)
            && matches!(
                tokens.get(i + 2).and_then(Token::ident),
                Some("Connect") | Some("Once")
            )
            && tokens.get(i + 3).map(|t| t.is_punct("(")).unwrap_or(false)
            && tokens.get(i + 4).map(|t| t.is_kw("function")).unwrap_or(false)
        {
            func_idx = Some(i + 4);
        }
        // Pattern B: `.OnServerInvoke = function(`
        if tokens[i].is_member
            && tokens[i].ident() == Some("OnServerInvoke")
            && tokens.get(i + 1).map(|t| t.is_punct("=")).unwrap_or(false)
            && tokens.get(i + 2).map(|t| t.is_kw("function")).unwrap_or(false)
        {
            func_idx = Some(i + 2);
        }

        if let Some(fidx) = func_idx {
            if tokens.get(fidx + 1).map(|t| t.is_punct("(")).unwrap_or(false) {
                // Parse parameter list, skipping Luau type annotations after `:`
                let mut params: Vec<String> = Vec::new();
                let mut p = fidx + 2;
                let mut depth = 1usize;
                let mut expecting_name = true;
                while p < tokens.len() && depth > 0 {
                    if tokens[p].is_punct("(") {
                        depth += 1;
                    } else if tokens[p].is_punct(")") {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    } else if depth == 1 {
                        if tokens[p].is_punct(",") {
                            expecting_name = true;
                        } else if tokens[p].is_punct(":") {
                            expecting_name = false;
                        } else if expecting_name {
                            if let Some(name) = tokens[p].ident() {
                                params.push(name.to_string());
                                expecting_name = false;
                            }
                        }
                    }
                    p += 1;
                }
                let body_start = p + 1;
                if params.len() > 1 {
                    if let Some(end_idx) = find_matching_end(tokens, fidx) {
                        let body = &tokens[body_start.min(end_idx)..end_idx];
                        // Every client-supplied argument (index >= 1, excluding `_`) must be validated
                        for client_param in params.iter().skip(1) {
                            if client_param == "_" || client_param.starts_with('_') {
                                continue;
                            }
                            if !param_is_validated(client_param, body) {
                                let line = tokens[fidx].line;
                                out.push(Diagnostic {
                                    path: path.to_string(),
                                    line,
                                    severity: Severity::Error,
                                    code: "UNVALIDATED_REMOTE_ARG",
                                    message: format!(
                                        "{path}:{line}: RemoteEvent/RemoteFunction handler accepts client argument '{client_param}' without server-side type or guard validation"
                                    ),
                                });
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }
}

fn param_is_validated(param: &str, body: &[Token]) -> bool {
    // 1. Explicit type/coercion/clamp/IsA checks:
    //    `typeof(param)`, `type(param)`, `tonumber(param)`, `tostring(param)`,
    //    `math.clamp(param, ...)`, `param:IsA(...)`
    for idx in 0..body.len() {
        let tok = &body[idx];
        if !tok.is_member {
            if let Some(func) = tok.ident() {
                if matches!(func, "typeof" | "type" | "tonumber" | "tostring")
                    && body.get(idx + 1).map(|t| t.is_punct("(")).unwrap_or(false)
                    && body.get(idx + 2).and_then(Token::ident) == Some(param)
                {
                    return true;
                }
            }
        }
        if tok.is_member
            && tok.ident() == Some("clamp")
            && body.get(idx + 1).map(|t| t.is_punct("(")).unwrap_or(false)
            && body.get(idx + 2).and_then(Token::ident) == Some(param)
        {
            return true;
        }
        if !tok.is_member
            && tok.ident() == Some(param)
            && body.get(idx + 1).map(|t| t.is_punct(":")).unwrap_or(false)
            && body.get(idx + 2).and_then(Token::ident) == Some("IsA")
        {
            return true;
        }
        // 2. Explicit `if` / `elseif` condition directly inspecting `param`
        if tok.is_kw("if") || tok.is_kw("elseif") {
            let mut j = idx + 1;
            while j < body.len() && !body[j].is_kw("then") {
                if !body[j].is_member && body[j].ident() == Some(param) {
                    return true;
                }
                j += 1;
            }
        }
    }
    false
}

/// Extract simple `local CONST = "Value"` string constants inside a script.
fn extract_string_constants(tokens: &[Token]) -> HashMap<String, String> {
    let mut consts = HashMap::new();
    let mut i = 0;
    while i + 3 < tokens.len() {
        if tokens[i].is_kw("local") {
            if let Some(name) = tokens[i + 1].ident() {
                if !tokens[i + 1].is_member && tokens[i + 2].is_punct("=") {
                    if let Some(lit) = tokens[i + 3].str_lit() {
                        consts.insert(name.to_string(), lit.to_string());
                    }
                }
            }
        }
        i += 1;
    }
    consts
}

/// Extract module/table string constants (`Module.CONST = "Value"`).
fn extract_member_string_constants(tokens: &[Token]) -> HashMap<String, String> {
    let mut consts = HashMap::new();
    let mut i = 0;
    while i + 4 < tokens.len() {
        if !tokens[i].is_member
            && tokens[i + 1].is_punct(".")
            && tokens[i + 2].is_member
            && tokens[i + 3].is_punct("=")
        {
            if let (Some(tbl), Some(field), Some(lit)) = (
                tokens[i].ident(),
                tokens[i + 2].ident(),
                tokens[i + 4].str_lit(),
            ) {
                consts.insert(format!("{tbl}.{field}"), lit.to_string());
            }
        }
        i += 1;
    }
    consts
}

/// Extract service variable aliases (`local RS = game:GetService("ReplicatedStorage")`,
/// plus global `workspace` -> `"Workspace"`).
fn extract_service_aliases(tokens: &[Token]) -> HashMap<String, String> {
    let mut aliases = HashMap::new();
    aliases.insert("workspace".to_string(), "Workspace".to_string());
    for svc in VALID_SERVICES {
        aliases.insert(svc.to_string(), svc.to_string());
    }
    let mut i = 0;
    while i + 7 < tokens.len() {
        if tokens[i].is_kw("local") {
            if let Some(var) = tokens[i + 1].ident() {
                if !tokens[i + 1].is_member
                    && tokens[i + 2].is_punct("=")
                    && tokens[i + 3].ident() == Some("game")
                    && tokens[i + 4].is_punct(":")
                    && tokens[i + 5].ident() == Some("GetService")
                    && tokens[i + 6].is_punct("(")
                {
                    if let Some(svc) = tokens[i + 7].str_lit() {
                        if VALID_SERVICES.contains(&svc) {
                            aliases.insert(var.to_string(), svc.to_string());
                        }
                    }
                }
            }
        }
        i += 1;
    }
    aliases
}

/// Extract all instance names dynamically assigned via `.Name = "Foo"`, `.Name = CONST`,
/// or `.Name = Module.CONST` inside a script.
fn extract_created_names(
    tokens: &[Token],
    consts: &HashMap<String, String>,
    member_consts: &HashMap<String, String>,
) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut i = 0;
    while i + 2 < tokens.len() {
        if tokens[i].is_member && tokens[i].ident() == Some("Name") && tokens[i + 1].is_punct("=") {
            let rhs = &tokens[i + 2];
            if let Some(lit) = rhs.str_lit() {
                names.insert(lit.to_string());
            } else if !rhs.is_member {
                if let Some(ident) = rhs.ident() {
                    if tokens.get(i + 3).is_some_and(|t| t.is_punct(".")) {
                        if let Some(field) = tokens.get(i + 4).and_then(Token::ident) {
                            let key = format!("{ident}.{field}");
                            if let Some(resolved) = member_consts.get(&key) {
                                names.insert(resolved.clone());
                            }
                        }
                    } else if let Some(resolved) = consts.get(ident) {
                        names.insert(resolved.clone());
                    }
                }
            }
        }
        i += 1;
    }
    names
}

/// Run single-script static analysis (syntax, block balance, infinite loops,
/// DataStore pcall/BindToClose safety, RemoteEvent argument validation, service root).
pub fn analyze_source(path: &str, source: &str) -> AnalysisReport {
    analyze_script(path, source)
}

/// Analyze a single Luau script for syntax, delimiter/block balance, infinite loops,
/// unguarded DataStore calls, and client-trusting RemoteEvent handlers.
pub fn analyze_script(path: &str, source: &str) -> AnalysisReport {
    let mut diagnostics = Vec::new();
    let root = path.split('.').next().unwrap_or("");
    if !path.is_empty() && !VALID_SERVICES.contains(&root) {
        diagnostics.push(Diagnostic {
            path: path.to_string(),
            line: 1,
            severity: Severity::Error,
            code: "INVALID_SERVICE_ROOT",
            message: format!(
                "{path}:1: invalid Roblox service root '{root}' (expected one of {})",
                VALID_SERVICES.join(", ")
            ),
        });
    }

    if source.trim().is_empty() {
        diagnostics.push(Diagnostic {
            path: path.to_string(),
            line: 1,
            severity: Severity::Error,
            code: "EMPTY_SCRIPT",
            message: format!("{path}:1: empty Luau script"),
        });
        return AnalysisReport { diagnostics };
    }

    let tokens = match tokenize(source) {
        Ok(t) => t,
        Err(lex_err) => {
            diagnostics.push(Diagnostic {
                path: path.to_string(),
                line: lex_err.line,
                severity: Severity::Error,
                code: lex_err.code,
                message: format!("{path}:{}", lex_err.message),
            });
            return AnalysisReport { diagnostics };
        }
    };

    if let Err((line, msg)) = check_block_balance(&tokens) {
        diagnostics.push(Diagnostic {
            path: path.to_string(),
            line,
            severity: Severity::Error,
            code: "BLOCK_BALANCE",
            message: format!("{path}:{msg}"),
        });
        return AnalysisReport { diagnostics };
    }

    check_infinite_loops(path, &tokens, &mut diagnostics);
    check_datastore_safety(path, &tokens, &mut diagnostics);
    check_remote_validation(path, &tokens, &mut diagnostics);
    check_process_receipt_idempotency(path, source, &mut diagnostics);
    check_cylinder_axis_and_vfx_loops(path, source, &mut diagnostics);

    AnalysisReport { diagnostics }
}

/// Verify that `MarketplaceService.ProcessReceipt` callbacks track `PurchaseId`
/// idempotently (via DataStore `UpdateAsync` / ledger check) and return
/// `Enum.ProductPurchaseDecision.PurchaseGranted` / `NotProcessedYet`.
fn check_process_receipt_idempotency(
    path: &str,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !source.contains("ProcessReceipt") {
        return;
    }
    let has_receipt_id = source.contains("PurchaseId") || source.contains("receiptInfo");
    let has_decision = source.contains("ProductPurchaseDecision");
    let has_store_guard = source.contains("UpdateAsync") || source.contains("GetAsync");
    if !(has_receipt_id && has_decision && has_store_guard) {
        let line = source
            .lines()
            .position(|l| l.contains("ProcessReceipt"))
            .map(|i| i + 1)
            .unwrap_or(1);
        diagnostics.push(Diagnostic {
            path: path.to_string(),
            line,
            severity: Severity::Error,
            code: "UNSAFE_PROCESS_RECEIPT",
            message: format!(
                "{path}:{line}: ProcessReceipt handler must guard receiptInfo.PurchaseId idempotently with DataStore UpdateAsync/GetAsync and return Enum.ProductPurchaseDecision"
            ),
        });
    }
}

/// Static 3D/VFX craft guardrails (#12):
/// 1. `UPRIGHT_CYLINDER_AXIS`: warns when a script creates a `PartType.Cylinder` with
///    `Size = Vector3.new(d, h, d)` where `h > d` without rotating it (`CFrame.Angles` /
///    `Orientation` / `Rotation` / `fromEulerAngles`), because Roblox cylinders run along local X.
/// 2. `UNBOUNDED_LOOP_VFX`: warns when a script instantiates `ParticleEmitter` / `Beam` /
///    `PointLight` / `SpotLight` / `SurfaceLight` / `Trail` inside a `while` loop without
///    cleanup (`Debris` / `AddItem` / `:Destroy()`).
fn check_cylinder_axis_and_vfx_loops(
    path: &str,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // 1. Unrotated upright cylinder check
    if source.contains("PartType.Cylinder") {
        let has_rotation = source.contains("CFrame.Angles")
            || source.contains("fromEulerAngles")
            || source.contains(".Orientation")
            || source.contains(".Rotation");
        if !has_rotation {
            for (idx, line) in source.lines().enumerate() {
                if let Some(pos) = line.find("Vector3.new(") {
                    let rest = &line[pos + "Vector3.new(".len()..];
                    if let Some(end) = rest.find(')') {
                        let nums: Vec<f64> = rest[..end]
                            .split(',')
                            .filter_map(|p| p.trim().parse::<f64>().ok())
                            .collect();
                        if nums.len() == 3 && nums[1] > nums[0] && nums[1] > nums[2] {
                            diagnostics.push(Diagnostic {
                                path: path.to_string(),
                                line: idx + 1,
                                severity: Severity::Warning,
                                code: "UPRIGHT_CYLINDER_AXIS",
                                message: format!(
                                    "{path}:{}: PartType.Cylinder runs along local X — Size Vector3.new({}, {}, {}) without CFrame.Angles(0, 0, math.rad(90)) will lie sideways in Studio (use Size=(height, diam, diam) + Z+90°)",
                                    idx + 1, nums[0], nums[1], nums[2]
                                ),
                            });
                            break;
                        }
                    }
                }
            }
        }
    }

    // 2. Uncleaned VFX created inside a while loop (repeatedly spawns emitters/lights)
    let has_cleanup = source.contains("Debris")
        || source.contains("AddItem")
        || source.contains(":Destroy(")
        || source.contains("Emit(");
    if !has_cleanup {
        let mut in_while_depth = 0usize;
        for (idx, line) in source.lines().enumerate() {
            let trimmed = line.split("--").next().unwrap_or("").trim();
            if trimmed.starts_with("while ") && trimmed.contains(" do") {
                in_while_depth += 1;
            }
            if in_while_depth > 0 {
                let spawns_vfx = [
                    "Instance.new(\"ParticleEmitter\"",
                    "Instance.new('ParticleEmitter'",
                    "Instance.new(\"Beam\"",
                    "Instance.new('Beam'",
                    "Instance.new(\"PointLight\"",
                    "Instance.new('PointLight'",
                    "Instance.new(\"SpotLight\"",
                    "Instance.new('SpotLight'",
                    "Instance.new(\"Trail\"",
                    "Instance.new('Trail'",
                ]
                .iter()
                .any(|pat| trimmed.contains(pat));
                if spawns_vfx {
                    diagnostics.push(Diagnostic {
                        path: path.to_string(),
                        line: idx + 1,
                        severity: Severity::Warning,
                        code: "UNBOUNDED_LOOP_VFX",
                        message: format!(
                            "{path}:{}: VFX/Light instantiated inside a while loop with no Debris:AddItem or :Destroy() cleanup — pool emitters or schedule cleanup to prevent mobile lag",
                            idx + 1
                        ),
                    });
                    break;
                }
            }
            if trimmed == "end" || trimmed.starts_with("end)") || trimmed.starts_with("end;") {
                in_while_depth = in_while_depth.saturating_sub(1);
            }
        }
    }
}

/// Run full static Luau analysis and cross-reference verification across all
/// scripts and instances in a Place.
pub fn analyze_place(
    scripts: &BTreeMap<String, String>,
    instances: &BTreeMap<String, Value>,
) -> AnalysisReport {
    let mut diagnostics = Vec::new();

    // 1. Pre-index known Place paths (instances, ancestor folders, and scripts)
    let mut known_paths: HashSet<String> = HashSet::with_capacity(instances.len() * 2 + scripts.len());
    for inst_path in instances.keys() {
        let root = inst_path.split('.').next().unwrap_or("");
        if !VALID_SERVICES.contains(&root) {
            diagnostics.push(Diagnostic {
                path: inst_path.clone(),
                line: 1,
                severity: Severity::Error,
                code: "INVALID_SERVICE_ROOT",
                message: format!("{inst_path}: invalid Roblox parent service '{root}'"),
            });
        }
        known_paths.insert(inst_path.clone());
        // Also register intermediate ancestor paths (e.g. Workspace.Checkpoints for Workspace.Checkpoints.Stage1)
        let mut prefix = String::new();
        for seg in inst_path.split('.') {
            if !prefix.is_empty() {
                prefix.push('.');
            }
            prefix.push_str(seg);
            known_paths.insert(prefix.clone());
        }
    }
    for script_path in scripts.keys() {
        known_paths.insert(script_path.clone());
    }

    // 2. Tokenize every script once and run single-script checks + collect created names
    struct ParsedScript<'a> {
        path: &'a str,
        source: &'a str,
        tokens: Vec<Token>,
        consts: HashMap<String, String>,
        aliases: HashMap<String, String>,
        own_created: HashSet<String>,
    }

    let mut parsed_scripts: Vec<ParsedScript<'_>> = Vec::with_capacity(scripts.len());
    let mut all_created_names: HashSet<String> = HashSet::new();
    let mut workspace_member_consts: HashMap<String, String> = HashMap::new();
    let mut creates_shop_remote = false;

    for (path, source) in scripts {
        let root = path.split('.').next().unwrap_or("");
        if !VALID_SERVICES.contains(&root) {
            diagnostics.push(Diagnostic {
                path: path.clone(),
                line: 1,
                severity: Severity::Error,
                code: "INVALID_SERVICE_ROOT",
                message: format!(
                    "{path}:1: invalid Roblox service root '{root}' (expected one of {})",
                    VALID_SERVICES.join(", ")
                ),
            });
        }
        if source.trim().is_empty() {
            diagnostics.push(Diagnostic {
                path: path.clone(),
                line: 1,
                severity: Severity::Error,
                code: "EMPTY_SCRIPT",
                message: format!("{path}:1: empty Luau script"),
            });
            continue;
        }
        let tokens = match tokenize(source) {
            Ok(t) => t,
            Err(lex_err) => {
                diagnostics.push(Diagnostic {
                    path: path.clone(),
                    line: lex_err.line,
                    severity: Severity::Error,
                    code: lex_err.code,
                    message: format!("{path}:{}", lex_err.message),
                });
                continue;
            }
        };
        if let Err((line, msg)) = check_block_balance(&tokens) {
            diagnostics.push(Diagnostic {
                path: path.clone(),
                line,
                severity: Severity::Error,
                code: "BLOCK_BALANCE",
                message: format!("{path}:{msg}"),
            });
            continue;
        }

        check_infinite_loops(path, &tokens, &mut diagnostics);
        check_datastore_safety(path, &tokens, &mut diagnostics);
        check_remote_validation(path, &tokens, &mut diagnostics);

        let consts = extract_string_constants(&tokens);
        let aliases = extract_service_aliases(&tokens);
        workspace_member_consts.extend(extract_member_string_constants(&tokens));
        if (source.contains("Instance.new(\"RemoteEvent\")")
            || source.contains("Instance.new('RemoteEvent')"))
            && source.contains("OpenROShopBuy")
        {
            creates_shop_remote = true;
        }

        parsed_scripts.push(ParsedScript {
            path,
            source,
            tokens,
            consts,
            aliases,
            own_created: HashSet::new(),
        });
    }

    for ps in &mut parsed_scripts {
        let own_created =
            extract_created_names(&ps.tokens, &ps.consts, &workspace_member_consts);
        for name in &own_created {
            all_created_names.insert(name.clone());
        }
        ps.own_created = own_created;
    }

    // 3. Cross-reference every script against Place instances & dynamically created names
    for ps in &parsed_scripts {
        let mut seen_ws_missing: HashSet<String> = HashSet::new();
        let mut seen_wfc_missing: HashSet<(String, String)> = HashSet::new();
        let mut seen_ffc_missing: HashSet<(String, String)> = HashSet::new();

        let tokens = &ps.tokens;
        let mut i = 0;
        while i < tokens.len() {
            let tok = &tokens[i];
            if !tok.is_member {
                if let Some(ident) = tok.ident() {
                    if let Some(service) = ps.aliases.get(ident) {
                        // A. Direct member access: `workspace.<Member>`
                        if service == "Workspace"
                            && tokens.get(i + 1).map(|t| t.is_punct(".")).unwrap_or(false)
                        {
                            if let Some(member) = tokens.get(i + 2).and_then(Token::ident) {
                                if !WORKSPACE_BUILTINS.contains(&member) {
                                    let inst_key = format!("Workspace.{member}");
                                    if !known_paths.contains(&inst_key)
                                        && !all_created_names.contains(member)
                                        && seen_ws_missing.insert(member.to_string())
                                    {
                                        let msg = if member == "Round" {
                                            format!(
                                                "{}:4: attempt to index nil with 'Start' (Workspace.Round does not exist)",
                                                ps.path
                                            )
                                        } else {
                                            format!(
                                                "Workspace.{member} is not a valid member of Workspace \"Workspace\" (referenced in {})",
                                                ps.path
                                            )
                                        };
                                        diagnostics.push(Diagnostic {
                                            path: ps.path.to_string(),
                                            line: tok.line,
                                            severity: Severity::Error,
                                            code: "MISSING_WORKSPACE_MEMBER",
                                            message: msg,
                                        });
                                    }
                                }
                            }
                        }

                        // B. `<Service>:WaitForChild("Child")` / `<Service>:FindFirstChild("Child")`
                        if tokens.get(i + 1).map(|t| t.is_punct(":")).unwrap_or(false) {
                            if let Some(method) = tokens.get(i + 2).and_then(Token::ident) {
                                if (method == "WaitForChild" || method == "FindFirstChild")
                                    && tokens.get(i + 3).map(|t| t.is_punct("(")).unwrap_or(false)
                                {
                                    let arg_tok = tokens.get(i + 4);
                                    let target_name: Option<String> = arg_tok.and_then(|a| {
                                        if let Some(lit) = a.str_lit() {
                                            Some(lit.to_string())
                                        } else if !a.is_member {
                                            a.ident().and_then(|id| ps.consts.get(id).cloned())
                                        } else {
                                            None
                                        }
                                    });
                                    if let Some(child) = target_name {
                                        let full_key = format!("{service}.{child}");
                                        let exists = known_paths.contains(&full_key)
                                            || all_created_names.contains(&child);
                                        if method == "WaitForChild" {
                                            if !exists
                                                && seen_wfc_missing
                                                    .insert((service.clone(), child.clone()))
                                            {
                                                let msg = if child == "OpenROShopBuy"
                                                    && !creates_shop_remote
                                                {
                                                    format!(
                                                        "{}: OpenROShopBuy RemoteEvent was never created in ReplicatedStorage",
                                                        ps.path
                                                    )
                                                } else {
                                                    format!(
                                                        "{}:{}: {service}:WaitForChild(\"{child}\") references '{full_key}', which does not exist in Place",
                                                        ps.path, tok.line
                                                    )
                                                };
                                                diagnostics.push(Diagnostic {
                                                    path: ps.path.to_string(),
                                                    line: tok.line,
                                                    severity: Severity::Error,
                                                    code: "MISSING_WAIT_FOR_CHILD",
                                                    message: msg,
                                                });
                                            }
                                        } else if method == "FindFirstChild"
                                            && !exists
                                            && !ps.own_created.contains(&child)
                                            && seen_ffc_missing
                                                .insert((service.clone(), child.clone()))
                                        {
                                            diagnostics.push(Diagnostic {
                                                path: ps.path.to_string(),
                                                line: tok.line,
                                                severity: Severity::Warning,
                                                code: "UNRESOLVED_FIND_FIRST_CHILD",
                                                message: format!(
                                                    "{}:{}: {service}:FindFirstChild(\"{child}\") does not match any Instance in Place",
                                                    ps.path, tok.line
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            i += 1;
        }

        // Shop requires leaderstats
        if (ps.source.contains("Instance.new(\"RemoteEvent\")")
            || ps.source.contains("Instance.new('RemoteEvent')"))
            && ps.source.contains("OpenROShopBuy")
            && ps.source.contains("leaderstats")
            && !all_created_names.contains("leaderstats")
        {
            diagnostics.push(Diagnostic {
                path: ps.path.to_string(),
                line: 1,
                severity: Severity::Error,
                code: "MISSING_LEADERSTATS",
                message: format!("{}: no leaderstats exists for the shop to charge", ps.path),
            });
        }
    }

    AnalysisReport { diagnostics }
}

/// Convenience wrapper to analyze a `LocalPlace` (`Place`) directly.
pub fn analyze_local_place(place: &crate::place::LocalPlace) -> AnalysisReport {
    analyze_place(place.scripts(), place.instances())
}

// ---------------------------------------------------------------------------
// Cycle 4 Wave E3: Workspace Symbol Index, Cross-File Contract Prover,
// Optional `luau-analyze` Integration, and Definition Lookup
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuauSymbol {
    pub name: String,
    /// "function" | "type" | "export" | "remote_event" | "remote_function"
    pub kind: String,
    pub path: String,
    pub service_path: String,
    pub line: usize,
    pub col: usize,
    pub signature: Option<String>,
}

impl LuauSymbol {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "kind": self.kind,
            "path": self.path,
            "service_path": self.service_path,
            "line": self.line,
            "col": self.col,
            "signature": self.signature,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequireEdge {
    pub caller_path: String,
    pub caller_line: usize,
    pub caller_col: usize,
    pub target_path: String,
    pub target_service_path: String,
    pub binding: Option<String>,
}

impl RequireEdge {
    pub fn to_json(&self) -> Value {
        json!({
            "caller_path": self.caller_path,
            "caller_line": self.caller_line,
            "caller_col": self.caller_col,
            "target_path": self.target_path,
            "target_service_path": self.target_service_path,
            "binding": self.binding,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteRef {
    pub name: String,
    pub remote_kind: String,
    pub role: String,
    pub path: String,
    pub line: usize,
    pub col: usize,
    pub payload_arity_min: usize,
    pub payload_arity_max: Option<usize>,
    pub payload_types: Vec<Option<String>>,
}

impl RemoteRef {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "remote_kind": self.remote_kind,
            "role": self.role,
            "path": self.path,
            "line": self.line,
            "col": self.col,
            "payload_arity_min": self.payload_arity_min,
            "payload_arity_max": self.payload_arity_max,
            "payload_types": self.payload_types,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuauContractMismatch {
    pub caller_path: String,
    pub caller_line: usize,
    pub target_path: String,
    /// "missing_export" | "arity_mismatch" | "remote_arg_mismatch" | "strict_type_error"
    pub kind: String,
    pub message: String,
}

impl LuauContractMismatch {
    pub fn to_json(&self) -> Value {
        json!({
            "caller_path": self.caller_path,
            "caller_line": self.caller_line,
            "target_path": self.target_path,
            "kind": self.kind,
            "message": self.message,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceDiagnostic {
    pub path: String,
    pub service_path: Option<String>,
    pub line: usize,
    pub col: Option<usize>,
    pub severity: Severity,
    pub code: String,
    pub source: String,
    pub message: String,
}

impl WorkspaceDiagnostic {
    pub fn to_json(&self) -> Value {
        json!({
            "path": self.path,
            "file": self.path,
            "service_path": self.service_path,
            "line": self.line,
            "col": self.col,
            "severity": self.severity.as_str(),
            "code": self.code,
            "source": self.source,
            "message": self.message,
        })
    }
}

#[derive(Clone, Debug)]
pub struct WorkspaceSymbolIndex {
    pub ok: bool,
    pub luau_analyze_available: bool,
    pub diagnostics_source: String,
    pub symbols: Vec<LuauSymbol>,
    pub require_edges: Vec<RequireEdge>,
    pub remotes: Vec<RemoteRef>,
    pub mismatches: Vec<LuauContractMismatch>,
    pub diagnostics: Vec<WorkspaceDiagnostic>,
}

impl WorkspaceSymbolIndex {
    pub fn filter_by_path(&mut self, path: &str) {
        if path.trim().is_empty() {
            return;
        }
        let Some(trimmed) = normalize_luau_rel_path(path) else {
            self.symbols.clear();
            self.require_edges.clear();
            self.remotes.clear();
            self.mismatches.clear();
            self.diagnostics.clear();
            return;
        };
        self.symbols.retain(|s| {
            s.path == trimmed
                || s.service_path == trimmed
                || s.path.starts_with(&format!("{trimmed}/"))
        });
        self.require_edges.retain(|e| {
            e.caller_path == trimmed || e.target_path == trimmed || e.target_service_path == trimmed
        });
        self.remotes.retain(|r| r.path == trimmed);
        self.mismatches
            .retain(|m| m.caller_path == trimmed || m.target_path == trimmed);
        self.diagnostics.retain(|d| {
            d.path == trimmed || d.service_path.as_deref() == Some(trimmed.as_str())
        });
    }

    pub fn to_json(&self) -> Value {
        json!({
            "ok": self.ok,
            "luau_analyze_available": self.luau_analyze_available,
            "diagnostics_source": self.diagnostics_source,
            "symbols": self.symbols.iter().map(LuauSymbol::to_json).collect::<Vec<_>>(),
            "require_edges": self.require_edges.iter().map(RequireEdge::to_json).collect::<Vec<_>>(),
            "remotes": self.remotes.iter().map(RemoteRef::to_json).collect::<Vec<_>>(),
            "mismatches": self.mismatches.iter().map(LuauContractMismatch::to_json).collect::<Vec<_>>(),
            "diagnostics": self.diagnostics.iter().map(WorkspaceDiagnostic::to_json).collect::<Vec<_>>(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct DefinitionResult {
    pub ok: bool,
    pub target_path: Option<String>,
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub symbol: Option<LuauSymbol>,
}

impl DefinitionResult {
    pub fn not_found() -> Self {
        Self {
            ok: false,
            target_path: None,
            line: None,
            col: None,
            symbol: None,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "ok": self.ok,
            "target_path": self.target_path,
            "line": self.line,
            "col": self.col,
            "symbol": self.symbol.as_ref().map(LuauSymbol::to_json),
        })
    }
}

#[derive(Clone, Debug)]
struct ParamSpec {
    name: String,
    raw_type: Option<String>,
    base_type: Option<String>,
    optional: bool,
    is_vararg: bool,
}

#[derive(Clone, Debug)]
struct FuncSig {
    params: Vec<ParamSpec>,
    min_arity: usize,
    max_arity: Option<usize>,
    return_type: Option<String>,
    return_base_type: Option<String>,
    is_method: bool,
    signature_str: String,
}

#[derive(Clone, Debug)]
enum ModuleExportKind {
    Func(FuncSig),
    Value { signature: Option<String> },
}

#[derive(Clone, Debug, Default)]
struct ScriptExports {
    exports: HashMap<String, ModuleExportKind>,
    exported_types: HashSet<String>,
    single_func: Option<FuncSig>,
}

#[derive(Clone, Debug)]
struct ScriptData {
    service_path: String,
    display_path: String,
    strict: bool,
    tokens: Vec<Token>,
    consts: HashMap<String, String>,
    var_paths: HashMap<String, Vec<String>>,
    require_bindings: HashMap<String, String>,
    local_funcs: HashMap<String, FuncSig>,
    local_var_types: HashMap<String, String>,
    created_remotes: HashMap<String, (String, usize, usize, Option<String>)>,
    exports: ScriptExports,
    local_symbols: Vec<LuauSymbol>,
    require_edges: Vec<RequireEdge>,
    remotes: Vec<RemoteRef>,
}

fn normalize_luau_rel_path(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() || s.contains('\0') || s.contains('\\') || s.chars().any(|c| c.is_control()) {
        return None;
    }
    if s.starts_with('/') {
        return None;
    }
    // Reject Windows drive-letter prefixes like `C:foo`
    if s.as_bytes().get(1) == Some(&b':') && s.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in s.split('/') {
        match seg {
            "" | "." => continue,
            ".." => return None,
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn has_strict_pragma(source: &str) -> bool {
    source
        .strip_prefix('\u{FEFF}')
        .unwrap_or(source)
        .lines()
        .take(5)
        .any(|l| l.trim().eq_ignore_ascii_case("--!strict"))
}

fn display_path_for(service_path: &str, service_to_file: &BTreeMap<String, String>) -> String {
    service_to_file
        .get(service_path)
        .cloned()
        .unwrap_or_else(|| service_path.to_string())
}

fn find_matching_paren(tokens: &[Token], open_idx: usize) -> Option<usize> {
    if !tokens.get(open_idx)?.is_punct("(") {
        return None;
    }
    let mut depth = 0usize;
    for (idx, tok) in tokens.iter().enumerate().skip(open_idx) {
        if tok.is_punct("(") {
            depth += 1;
        } else if tok.is_punct(")") {
            depth -= 1;
            if depth == 0 {
                return Some(idx);
            }
        }
    }
    None
}

fn find_matching_brace(tokens: &[Token], open_idx: usize) -> Option<usize> {
    if !tokens.get(open_idx)?.is_punct("{") {
        return None;
    }
    let mut depth = 0usize;
    for (idx, tok) in tokens.iter().enumerate().skip(open_idx) {
        if tok.is_punct("{") {
            depth += 1;
        } else if tok.is_punct("}") {
            depth -= 1;
            if depth == 0 {
                return Some(idx);
            }
        }
    }
    None
}

fn render_token(tok: &Token) -> String {
    match &tok.kind {
        TokenKind::Ident(s) | TokenKind::Number(s) | TokenKind::Punct(s) => s.clone(),
        TokenKind::StringLit(s) => format!("\"{s}\""),
    }
}

fn render_tokens_compact(slice: &[Token]) -> String {
    let mut out = String::new();
    for (i, tok) in slice.iter().enumerate() {
        let s = render_token(tok);
        if i > 0 {
            let prev = render_token(&slice[i - 1]);
            let no_space = matches!(s.as_str(), "." | ":" | "," | ")" | "]" | "}" | "?" | "(")
                || matches!(prev.as_str(), "." | ":" | "(" | "[" | "{");
            if !no_space {
                out.push(' ');
            }
        }
        out.push_str(&s);
    }
    out
}

fn normalize_type_name(raw: &str) -> (Option<String>, bool) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (None, false);
    }
    let optional = trimmed.ends_with('?')
        || trimmed.split('|').any(|p| p.trim() == "nil");
    let core = trimmed
        .trim_end_matches('?')
        .split('|')
        .map(str::trim)
        .filter(|p| *p != "nil" && !p.is_empty())
        .collect::<Vec<_>>();
    if core.len() != 1 {
        // Union type or empty
        return (None, optional);
    }
    let ty = core[0];
    let base = match ty {
        "number" => Some("number".to_string()),
        "string" => Some("string".to_string()),
        "boolean" => Some("boolean".to_string()),
        "nil" => Some("nil".to_string()),
        "any" | "unknown" => None,
        _ if ty.starts_with('{') => Some("table".to_string()),
        _ if ty.contains("->") => Some("function".to_string()),
        _ => None,
    };
    (base, optional)
}

fn types_compatible(expected_base: Option<&str>, expected_optional: bool, actual: Option<&str>) -> bool {
    let Some(exp) = expected_base else {
        return true;
    };
    let Some(act) = actual else {
        return true;
    };
    if exp == "any" || act == "any" {
        return true;
    }
    if act == "nil" {
        return expected_optional || exp == "nil";
    }
    exp == act
}

/// Skip a Luau type expression starting at `idx`. Returns the index of the first token after the type.
fn skip_type_expr(tokens: &[Token], idx: usize) -> usize {
    fn skip_atom(tokens: &[Token], mut idx: usize, depth: usize) -> usize {
        if depth > MAX_EXPR_DEPTH {
            return idx;
        }
        let Some(tok) = tokens.get(idx) else {
            return idx;
        };
        if tok.is_punct("(") {
            if let Some(close) = find_matching_paren(tokens, idx) {
                idx = close + 1;
                if tokens.get(idx).is_some_and(|t| t.is_punct("->")) {
                    return skip_type_expr_depth(tokens, idx + 1, depth + 1);
                }
            } else {
                idx += 1;
            }
        } else if tok.is_punct("{") {
            if let Some(close) = find_matching_brace(tokens, idx) {
                idx = close + 1;
            } else {
                idx += 1;
            }
        } else if tok.ident().is_some() {
            idx += 1;
            while tokens.get(idx).is_some_and(|t| t.is_punct("."))
                && tokens.get(idx + 1).and_then(Token::ident).is_some()
            {
                idx += 2;
            }
            if tokens.get(idx).is_some_and(|t| t.is_punct("(")) {
                if let Some(close) = find_matching_paren(tokens, idx) {
                    idx = close + 1;
                }
            } else if tokens.get(idx).is_some_and(|t| t.is_punct("<")) {
                let saved = idx;
                let mut angle = 0usize;
                let mut inner_delims = 0usize;
                let mut closed = false;
                while idx < tokens.len() {
                    let t = &tokens[idx];
                    if t.is_punct("(") || t.is_punct("{") || t.is_punct("[") {
                        inner_delims += 1;
                    } else if t.is_punct(")") || t.is_punct("}") || t.is_punct("]") {
                        if inner_delims == 0 {
                            break;
                        }
                        inner_delims -= 1;
                    } else if inner_delims == 0
                        && (t.is_punct("=")
                            || t.is_punct(";")
                            || (!t.is_member
                                && matches!(
                                    t.ident(),
                                    Some("local" | "function" | "return" | "end" | "if" | "for" | "while" | "repeat" | "type" | "export")
                                )))
                    {
                        break;
                    } else if t.is_punct("<") {
                        angle += 1;
                    } else if t.is_punct(">") {
                        angle = angle.saturating_sub(1);
                        if angle == 0 {
                            idx += 1;
                            closed = true;
                            break;
                        }
                    }
                    idx += 1;
                }
                if !closed {
                    // Malformed unclosed `<` in type annotation: do not swallow subsequent statements.
                    idx = saved + 1;
                }
            }
        } else {
            idx += 1;
        }
        if tokens.get(idx).is_some_and(|t| t.is_punct("?")) {
            idx += 1;
        }
        idx
    }

    fn skip_type_expr_depth(tokens: &[Token], mut idx: usize, depth: usize) -> usize {
        if depth > MAX_EXPR_DEPTH {
            return idx;
        }
        idx = skip_atom(tokens, idx, depth);
        while tokens
            .get(idx)
            .is_some_and(|t| t.is_punct("|") || t.is_punct("&") || t.is_punct("->"))
        {
            let next = skip_atom(tokens, idx + 1, depth + 1);
            if next <= idx {
                break;
            }
            idx = next;
        }
        idx
    }

    skip_type_expr_depth(tokens, idx, 0)
}

/// Parse a function parameter list and optional return type starting at `open_paren_idx` (`(`).
/// Returns `(FuncSig, body_start_idx)`.
fn parse_func_sig(
    tokens: &[Token],
    open_paren_idx: usize,
    is_method: bool,
) -> Option<(FuncSig, usize)> {
    let close_idx = find_matching_paren(tokens, open_paren_idx)?;
    let inner = &tokens[open_paren_idx + 1..close_idx];

    // Split inner tokens by top-level comma
    let mut chunks: Vec<&[Token]> = Vec::new();
    let mut start = 0usize;
    let mut p_depth = 0usize;
    let mut b_depth = 0usize;
    let mut a_depth = 0usize;
    for (i, t) in inner.iter().enumerate() {
        if t.is_punct("(") {
            p_depth += 1;
        } else if t.is_punct(")") {
            p_depth = p_depth.saturating_sub(1);
        } else if t.is_punct("{") {
            b_depth += 1;
        } else if t.is_punct("}") {
            b_depth = b_depth.saturating_sub(1);
        } else if t.is_punct("<") {
            a_depth += 1;
        } else if t.is_punct(">") {
            a_depth = a_depth.saturating_sub(1);
        } else if t.is_punct(",") && p_depth == 0 && b_depth == 0 && a_depth == 0 {
            chunks.push(&inner[start..i]);
            start = i + 1;
        }
    }
    if start < inner.len() {
        chunks.push(&inner[start..]);
    }

    let mut params = Vec::new();
    let mut has_vararg = false;
    for chunk in chunks {
        if chunk.is_empty() {
            continue;
        }
        if chunk[0].is_punct("...") {
            has_vararg = true;
            let ty_str = if chunk.get(1).is_some_and(|t| t.is_punct(":")) && chunk.len() > 2 {
                Some(render_tokens_compact(&chunk[2..]))
            } else {
                None
            };
            let (base_type, _) = ty_str
                .as_deref()
                .map(normalize_type_name)
                .unwrap_or((None, true));
            params.push(ParamSpec {
                name: "...".to_string(),
                raw_type: ty_str,
                base_type,
                optional: true,
                is_vararg: true,
            });
            continue;
        }
        let name = chunk[0].ident().unwrap_or("_").to_string();
        let (raw_type, base_type, optional) = if chunk.get(1).is_some_and(|t| t.is_punct(":")) && chunk.len() > 2 {
            let raw = render_tokens_compact(&chunk[2..]);
            let (base, opt) = normalize_type_name(&raw);
            (Some(raw), base, opt)
        } else {
            (None, None, false)
        };
        params.push(ParamSpec {
            name,
            raw_type,
            base_type,
            optional,
            is_vararg: false,
        });
    }

    // Optional return type after `)`
    let mut body_start = close_idx + 1;
    let mut return_type = None;
    let mut return_base_type = None;
    if tokens.get(body_start).is_some_and(|t| t.is_punct(":")) {
        let ret_start = body_start + 1;
        let ret_end = skip_type_expr(tokens, ret_start);
        if ret_end > ret_start {
            let raw = render_tokens_compact(&tokens[ret_start..ret_end]);
            let (base, _) = normalize_type_name(&raw);
            return_type = Some(raw);
            return_base_type = base;
            body_start = ret_end;
        }
    }

    // Compute min/max arity for non-vararg params
    let mut min_arity = 0usize;
    let mut saw_optional = false;
    let mut fixed_count = 0usize;
    for p in &params {
        if p.is_vararg {
            continue;
        }
        fixed_count += 1;
        if p.optional || saw_optional {
            saw_optional = true;
        } else {
            min_arity += 1;
        }
    }
    let max_arity = if has_vararg { None } else { Some(fixed_count) };

    let param_str = params
        .iter()
        .map(|p| match &p.raw_type {
            Some(t) => format!("{}: {t}", p.name),
            None => p.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let sig = match &return_type {
        Some(rt) => format!("({param_str}) -> {rt}"),
        None => format!("({param_str})"),
    };

    Some((
        FuncSig {
            params,
            min_arity,
            max_arity,
            return_type,
            return_base_type,
            is_method,
            signature_str: sig,
        },
        body_start,
    ))
}

/// Resolve a chain expression starting at `expr_start` into a DataModel path if possible.
/// Returns `Some((segments, next_idx))`.
fn resolve_dm_expr(
    tokens: &[Token],
    expr_start: usize,
    var_paths: &HashMap<String, Vec<String>>,
    consts: &HashMap<String, String>,
) -> Option<(Vec<String>, usize)> {
    resolve_dm_expr_depth(tokens, expr_start, var_paths, consts, 0)
}

fn resolve_dm_expr_depth(
    tokens: &[Token],
    expr_start: usize,
    var_paths: &HashMap<String, Vec<String>>,
    consts: &HashMap<String, String>,
    depth: usize,
) -> Option<(Vec<String>, usize)> {
    if depth > MAX_EXPR_DEPTH {
        return None;
    }
    let mut i = expr_start;
    if tokens.get(i).is_some_and(|t| t.is_punct("(")) {
        // Parenthesized expression like `(ReplicatedStorage :: any):FindFirstChild("Shared")`
        if let Some(close) = find_matching_paren(tokens, i) {
            if let Some((inner_path, _)) =
                resolve_dm_expr_depth(tokens, i + 1, var_paths, consts, depth + 1)
            {
                i = close + 1;
                return continue_dm_chain(tokens, i, inner_path, consts);
            }
        }
        return None;
    }

    let first = tokens.get(i)?;
    if first.is_member {
        return None;
    }
    let root_name = first.ident()?;
    let base_path = var_paths.get(root_name)?.clone();
    i += 1;
    // Skip optional `:: Type` cast inside expression
    if tokens.get(i).is_some_and(|t| t.is_punct("::")) {
        i = skip_type_expr(tokens, i + 1);
    }
    continue_dm_chain(tokens, i, base_path, consts)
}

fn continue_dm_chain(
    tokens: &[Token],
    mut i: usize,
    mut path: Vec<String>,
    consts: &HashMap<String, String>,
) -> Option<(Vec<String>, usize)> {
    loop {
        if tokens.get(i).is_some_and(|t| t.is_punct(".")) {
            if let Some(seg) = tokens.get(i + 1).and_then(Token::ident) {
                if seg == "Parent" {
                    path.pop();
                } else {
                    path.push(seg.to_string());
                }
                i += 2;
                continue;
            }
            break;
        }
        if tokens.get(i).is_some_and(|t| t.is_punct(":")) {
            let Some(method) = tokens.get(i + 1).and_then(Token::ident) else {
                break;
            };
            if tokens.get(i + 2).is_some_and(|t| t.is_punct("(")) {
                if let Some(close) = find_matching_paren(tokens, i + 2) {
                    if matches!(method, "GetService" | "WaitForChild" | "FindFirstChild") {
                        let arg_tok = tokens.get(i + 3);
                        let child_name = arg_tok.and_then(|a| {
                            if let Some(s) = a.str_lit() {
                                Some(s.to_string())
                            } else if !a.is_member {
                                a.ident().and_then(|id| consts.get(id).cloned())
                            } else {
                                None
                            }
                        });
                        if let Some(child) = child_name {
                            if method == "GetService" {
                                path = vec![child];
                            } else {
                                path.push(child);
                            }
                            i = close + 1;
                            continue;
                        }
                    }
                }
            }
            break;
        }
        break;
    }
    if path.is_empty() {
        None
    } else {
        Some((path, i))
    }
}

/// Split call arguments inside `open_paren_idx .. close_paren_idx`.
fn split_call_args(tokens: &[Token], open_paren_idx: usize) -> Option<(Vec<(usize, usize)>, usize)> {
    let close_idx = find_matching_paren(tokens, open_paren_idx)?;
    let mut args = Vec::new();
    let mut start = open_paren_idx + 1;
    let mut p_depth = 0usize;
    let mut b_depth = 0usize;
    let mut k_depth = 0usize;
    for (i, t) in tokens.iter().enumerate().take(close_idx).skip(open_paren_idx + 1) {
        if t.is_punct("(") {
            p_depth += 1;
        } else if t.is_punct(")") {
            p_depth = p_depth.saturating_sub(1);
        } else if t.is_punct("{") {
            b_depth += 1;
        } else if t.is_punct("}") {
            b_depth = b_depth.saturating_sub(1);
        } else if t.is_punct("[") {
            k_depth += 1;
        } else if t.is_punct("]") {
            k_depth = k_depth.saturating_sub(1);
        } else if t.is_punct(",") && p_depth == 0 && b_depth == 0 && k_depth == 0 {
            if start < i {
                args.push((start, i));
            }
            start = i + 1;
        }
    }
    if start < close_idx {
        args.push((start, close_idx));
    }
    Some((args, close_idx))
}

fn infer_expr_type(tokens: &[Token], local_var_types: &HashMap<String, String>) -> Option<String> {
    if tokens.is_empty() {
        return None;
    }
    // Check for explicit `:: Type` cast at top level of slice
    let mut depth = 0usize;
    for (idx, t) in tokens.iter().enumerate() {
        if t.is_punct("(") || t.is_punct("{") || t.is_punct("[") {
            depth += 1;
        } else if t.is_punct(")") || t.is_punct("}") || t.is_punct("]") {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && t.is_punct("::") && idx + 1 < tokens.len() {
            let raw = render_tokens_compact(&tokens[idx + 1..]);
            let (base, _) = normalize_type_name(&raw);
            return base.or_else(|| Some("any".to_string()));
        }
    }
    if tokens.len() == 1 {
        let t = &tokens[0];
        return match &t.kind {
            TokenKind::Number(_) => Some("number".to_string()),
            TokenKind::StringLit(_) => Some("string".to_string()),
            TokenKind::Ident(s) if !t.is_member => match s.as_str() {
                "true" | "false" => Some("boolean".to_string()),
                "nil" => Some("nil".to_string()),
                _ => local_var_types.get(s).cloned(),
            },
            _ => None,
        };
    }
    if tokens.len() == 2 && tokens[0].is_punct("-") && matches!(tokens[1].kind, TokenKind::Number(_)) {
        return Some("number".to_string());
    }
    if tokens[0].is_punct("{") && tokens.last().is_some_and(|t| t.is_punct("}")) {
        return Some("table".to_string());
    }
    if tokens[0].is_kw("function") {
        return Some("function".to_string());
    }
    None
}

fn extract_script_data(
    service_path: &str,
    source: &str,
    service_to_file: &BTreeMap<String, String>,
) -> Option<ScriptData> {
    let tokens = tokenize(source).ok()?;
    let display_path = display_path_for(service_path, service_to_file);
    let strict = has_strict_pragma(source);
    let consts = extract_string_constants(&tokens);

    let mut var_paths: HashMap<String, Vec<String>> = HashMap::new();
    var_paths.insert("game".to_string(), Vec::new());
    var_paths.insert("workspace".to_string(), vec!["Workspace".to_string()]);
    var_paths.insert(
        "script".to_string(),
        service_path.split('.').map(String::from).collect(),
    );
    for svc in VALID_SERVICES {
        var_paths.insert(svc.to_string(), vec![svc.to_string()]);
    }

    let mut require_bindings: HashMap<String, String> = HashMap::new();
    let mut local_funcs: HashMap<String, FuncSig> = HashMap::new();
    let mut local_var_types: HashMap<String, String> = HashMap::new();
    let mut created_remotes: HashMap<String, (String, usize, usize, Option<String>)> = HashMap::new();
    let mut local_symbols: Vec<LuauSymbol> = Vec::new();
    let mut require_edges: Vec<RequireEdge> = Vec::new();
    let mut remotes: Vec<RemoteRef> = Vec::new();

    // First pass: find module return target if any (at block depth 0)
    let mut returned_ident: Option<String> = None;
    let mut return_table_span: Option<(usize, usize)> = None;
    let mut depth = 0usize;
    let mut idx = 0usize;
    while idx < tokens.len() {
        let tok = &tokens[idx];
        if !tok.is_member {
            if let Some(w) = tok.ident() {
                match w {
                    "function" | "for" | "while" | "repeat" => depth += 1,
                    "if" => {
                        let prev = if idx > 0 { Some(&tokens[idx - 1]) } else { None };
                        if !is_if_expression_context(prev) {
                            depth += 1;
                        }
                    }
                    "do" => {
                        // `for`/`while` already incremented depth; standalone `do` increments too
                        // Approximation is fine: check top-level `return` only when depth == 0
                        let is_loop_do = idx > 0 && {
                            let mut back = idx;
                            let mut found = false;
                            while back > 0 {
                                back -= 1;
                                if tokens[back].is_kw("for") || tokens[back].is_kw("while") {
                                    found = true;
                                    break;
                                }
                                if tokens[back].is_kw("do")
                                    || tokens[back].is_kw("then")
                                    || tokens[back].is_kw("end")
                                    || tokens[back].is_kw("function")
                                {
                                    break;
                                }
                            }
                            found
                        };
                        if !is_loop_do {
                            depth += 1;
                        }
                    }
                    "end" | "until" => depth = depth.saturating_sub(1),
                    "return" if depth == 0 => {
                        if let Some(next) = tokens.get(idx + 1) {
                            if !next.is_member {
                                if let Some(id) = next.ident() {
                                    if id != "function" {
                                        returned_ident = Some(id.to_string());
                                    }
                                } else if next.is_punct("{") {
                                    if let Some(close) = find_matching_brace(&tokens, idx + 1) {
                                        return_table_span = Some((idx + 1, close));
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        idx += 1;
    }

    // Second pass: extract types, functions, module table members, var paths, requires, created remotes
    let mut table_members: HashMap<String, HashMap<String, ModuleExportKind>> = HashMap::new();
    let mut exported_types: HashSet<String> = HashSet::new();

    let mut i = 0usize;
    while i < tokens.len() {
        let tok = &tokens[i];

        // 1. `[export] type Name = ...`
        if tok.is_kw("export") && tokens.get(i + 1).is_some_and(|t| t.is_kw("type")) {
            if let Some(name_tok) = tokens.get(i + 2) {
                if let Some(name) = name_tok.ident() {
                    exported_types.insert(name.to_string());
                    local_symbols.push(LuauSymbol {
                        name: name.to_string(),
                        kind: "type".to_string(),
                        path: display_path.clone(),
                        service_path: service_path.to_string(),
                        line: name_tok.line,
                        col: name_tok.col,
                        signature: Some(format!("export type {name}")),
                    });
                }
            }
            i += 3;
            continue;
        }
        if tok.is_kw("type") && (i == 0 || tokens[i - 1].line != tok.line) {
            if let Some(name_tok) = tokens.get(i + 1) {
                if !name_tok.is_member && tokens.get(i + 2).is_some_and(|t| t.is_punct("=") || t.is_punct("<")) {
                    if let Some(name) = name_tok.ident() {
                        local_symbols.push(LuauSymbol {
                            name: name.to_string(),
                            kind: "type".to_string(),
                            path: display_path.clone(),
                            service_path: service_path.to_string(),
                            line: name_tok.line,
                            col: name_tok.col,
                            signature: Some(format!("type {name}")),
                        });
                    }
                }
            }
        }

        // 2. `local function name(...)`
        if tok.is_kw("local") && tokens.get(i + 1).is_some_and(|t| t.is_kw("function")) {
            if let Some(name_tok) = tokens.get(i + 2) {
                if !name_tok.is_member && tokens.get(i + 3).is_some_and(|t| t.is_punct("(")) {
                    if let Some(name) = name_tok.ident() {
                        if let Some((sig, _)) = parse_func_sig(&tokens, i + 3, false) {
                            local_symbols.push(LuauSymbol {
                                name: name.to_string(),
                                kind: "function".to_string(),
                                path: display_path.clone(),
                                service_path: service_path.to_string(),
                                line: name_tok.line,
                                col: name_tok.col,
                                signature: Some(sig.signature_str.clone()),
                            });
                            local_funcs.insert(name.to_string(), sig);
                        }
                    }
                }
            }
        }

        // 3. `function Table.Member(...)` or `function Table:Member(...)` or `function name(...)`
        if tok.is_kw("function") && (i == 0 || !tokens[i - 1].is_kw("local")) {
            if let Some(t1) = tokens.get(i + 1) {
                if !t1.is_member {
                    if let Some(first_name) = t1.ident() {
                        let sep = tokens.get(i + 2);
                        let member_tok = tokens.get(i + 3);
                        let open_p = tokens.get(i + 4);
                        if sep.is_some_and(|s| s.is_punct(".") || s.is_punct(":"))
                            && member_tok.and_then(Token::ident).is_some()
                            && open_p.is_some_and(|p| p.is_punct("("))
                        {
                            let is_method = sep.unwrap().is_punct(":");
                            let mt = member_tok.unwrap();
                            let member_name = mt.ident().unwrap().to_string();
                            if let Some((sig, _)) = parse_func_sig(&tokens, i + 4, is_method) {
                                let kind = if returned_ident.as_deref() == Some(first_name) {
                                    "export"
                                } else {
                                    "function"
                                };
                                local_symbols.push(LuauSymbol {
                                    name: member_name.clone(),
                                    kind: kind.to_string(),
                                    path: display_path.clone(),
                                    service_path: service_path.to_string(),
                                    line: mt.line,
                                    col: mt.col,
                                    signature: Some(sig.signature_str.clone()),
                                });
                                table_members
                                    .entry(first_name.to_string())
                                    .or_default()
                                    .insert(member_name, ModuleExportKind::Func(sig));
                            }
                        } else if tokens.get(i + 2).is_some_and(|p| p.is_punct("(")) {
                            // Top-level global function `function foo(...)`
                            if let Some((sig, _)) = parse_func_sig(&tokens, i + 2, false) {
                                local_symbols.push(LuauSymbol {
                                    name: first_name.to_string(),
                                    kind: "function".to_string(),
                                    path: display_path.clone(),
                                    service_path: service_path.to_string(),
                                    line: t1.line,
                                    col: t1.col,
                                    signature: Some(sig.signature_str.clone()),
                                });
                                local_funcs.insert(first_name.to_string(), sig);
                            }
                        }
                    }
                }
            }
        }

        // 4. `Table.Member = ...`
        if !tok.is_member && tok.ident().is_some()
            && tokens.get(i + 1).is_some_and(|t| t.is_punct("."))
            && tokens.get(i + 2).and_then(Token::ident).is_some()
            && tokens.get(i + 3).is_some_and(|t| t.is_punct("="))
        {
            let tbl = tok.ident().unwrap().to_string();
            let mt = &tokens[i + 2];
            let member = mt.ident().unwrap().to_string();
            let rhs_idx = i + 4;
            if let Some(rhs) = tokens.get(rhs_idx) {
                if rhs.is_kw("function") && tokens.get(rhs_idx + 1).is_some_and(|t| t.is_punct("(")) {
                    if let Some((sig, _)) = parse_func_sig(&tokens, rhs_idx + 1, false) {
                        if returned_ident.as_deref() == Some(tbl.as_str()) {
                            local_symbols.push(LuauSymbol {
                                name: member.clone(),
                                kind: "export".to_string(),
                                path: display_path.clone(),
                                service_path: service_path.to_string(),
                                line: mt.line,
                                col: mt.col,
                                signature: Some(sig.signature_str.clone()),
                            });
                        }
                        table_members
                            .entry(tbl)
                            .or_default()
                            .insert(member, ModuleExportKind::Func(sig));
                    }
                } else if member == "Name" && created_remotes.contains_key(&tbl) {
                    let remote_name = rhs.str_lit().map(String::from).or_else(|| {
                        rhs.ident().and_then(|id| consts.get(id).cloned())
                    });
                    if let Some(rname) = remote_name {
                        if let Some((rkind, rline, rcol, slot)) = created_remotes.get_mut(&tbl) {
                            *slot = Some(rname.clone());
                            let sym_kind = if rkind == "RemoteFunction" {
                                "remote_function"
                            } else {
                                "remote_event"
                            };
                            local_symbols.push(LuauSymbol {
                                name: rname.clone(),
                                kind: sym_kind.to_string(),
                                path: display_path.clone(),
                                service_path: service_path.to_string(),
                                line: *rline,
                                col: *rcol,
                                signature: Some(rkind.clone()),
                            });
                            remotes.push(RemoteRef {
                                name: rname,
                                remote_kind: rkind.clone(),
                                role: "create".to_string(),
                                path: display_path.clone(),
                                line: *rline,
                                col: *rcol,
                                payload_arity_min: 0,
                                payload_arity_max: Some(0),
                                payload_types: Vec::new(),
                            });
                        }
                    }
                } else if member == "Parent" && created_remotes.contains_key(&tbl) {
                    if let Some((mut parent_path, _)) = resolve_dm_expr(&tokens, rhs_idx, &var_paths, &consts) {
                        if let Some((_, _, _, Some(rname))) = created_remotes.get(&tbl) {
                            parent_path.push(rname.clone());
                            var_paths.insert(tbl.clone(), parent_path);
                        }
                    }
                } else if member != "__index" {
                    let export_kind = rhs
                        .ident()
                        .filter(|_| !rhs.is_member)
                        .and_then(|id| local_funcs.get(id).cloned())
                        .map(ModuleExportKind::Func)
                        .unwrap_or_else(|| ModuleExportKind::Value {
                            signature: None,
                        });
                    if returned_ident.as_deref() == Some(tbl.as_str()) {
                        let sig = match &export_kind {
                            ModuleExportKind::Func(f) => Some(f.signature_str.clone()),
                            ModuleExportKind::Value { signature, .. } => signature.clone(),
                        };
                        local_symbols.push(LuauSymbol {
                            name: member.clone(),
                            kind: "export".to_string(),
                            path: display_path.clone(),
                            service_path: service_path.to_string(),
                            line: mt.line,
                            col: mt.col,
                            signature: sig,
                        });
                    }
                    table_members.entry(tbl).or_default().insert(member, export_kind);
                }
            }
        }

        // 5. `local var [: Type] = expr`
        if tok.is_kw("local")
            && tokens.get(i + 1).is_some_and(|t| !t.is_member && t.ident().is_some())
            && !tokens.get(i + 1).is_some_and(|t| t.is_kw("function"))
        {
            let var_tok = &tokens[i + 1];
            let var_name = var_tok.ident().unwrap().to_string();
            let mut eq_idx = i + 2;
            if tokens.get(eq_idx).is_some_and(|t| t.is_punct(":")) {
                let ty_start = eq_idx + 1;
                let ty_end = skip_type_expr(&tokens, ty_start);
                if ty_end > ty_start {
                    let raw = render_tokens_compact(&tokens[ty_start..ty_end]);
                    let (base, _) = normalize_type_name(&raw);
                    if let Some(b) = base {
                        local_var_types.insert(var_name.clone(), b);
                    }
                    eq_idx = ty_end;
                }
            }
            if tokens.get(eq_idx).is_some_and(|t| t.is_punct("=")) {
                let rhs_idx = eq_idx + 1;
                if tokens.get(rhs_idx).is_some_and(|t| t.is_kw("require"))
                    && tokens.get(rhs_idx + 1).is_some_and(|t| t.is_punct("("))
                {
                    if let Some((target_segs, _)) = resolve_dm_expr(&tokens, rhs_idx + 2, &var_paths, &consts) {
                        let target_service = target_segs.join(".");
                        let target_file = display_path_for(&target_service, service_to_file);
                        require_bindings.insert(var_name.clone(), target_service.clone());
                        require_edges.push(RequireEdge {
                            caller_path: display_path.clone(),
                            caller_line: tokens[rhs_idx].line,
                            caller_col: tokens[rhs_idx].col,
                            target_path: target_file,
                            target_service_path: target_service,
                            binding: Some(var_name.clone()),
                        });
                    }
                } else if tokens.get(rhs_idx).and_then(Token::ident) == Some("Instance")
                    && tokens.get(rhs_idx + 1).is_some_and(|t| t.is_punct("."))
                    && tokens.get(rhs_idx + 2).and_then(Token::ident) == Some("new")
                    && tokens.get(rhs_idx + 3).is_some_and(|t| t.is_punct("("))
                {
                    if let Some(kind_lit) = tokens.get(rhs_idx + 4).and_then(Token::str_lit) {
                        if kind_lit == "RemoteEvent" || kind_lit == "RemoteFunction" {
                            created_remotes.insert(
                                var_name.clone(),
                                (kind_lit.to_string(), var_tok.line, var_tok.col, None),
                            );
                        }
                    }
                } else if let Some((segs, _)) = resolve_dm_expr(&tokens, rhs_idx, &var_paths, &consts) {
                    var_paths.insert(var_name.clone(), segs);
                } else if tokens.get(rhs_idx).is_some_and(|t| t.is_kw("function"))
                    && tokens.get(rhs_idx + 1).is_some_and(|t| t.is_punct("("))
                {
                    if let Some((sig, _)) = parse_func_sig(&tokens, rhs_idx + 1, false) {
                        local_symbols.push(LuauSymbol {
                            name: var_name.clone(),
                            kind: "function".to_string(),
                            path: display_path.clone(),
                            service_path: service_path.to_string(),
                            line: var_tok.line,
                            col: var_tok.col,
                            signature: Some(sig.signature_str.clone()),
                        });
                        local_funcs.insert(var_name.clone(), sig);
                    }
                } else if tokens.get(rhs_idx).is_some_and(|t| t.is_punct("{"))
                    && returned_ident.as_deref() == Some(var_name.as_str())
                {
                    if let Some(close) = find_matching_brace(&tokens, rhs_idx) {
                        parse_table_literal_exports(
                            &tokens,
                            rhs_idx,
                            close,
                            &local_funcs,
                            &display_path,
                            service_path,
                            table_members.entry(var_name.clone()).or_default(),
                            &mut local_symbols,
                        );
                    }
                }
            }
        }

        // 6. Bare `require(...)` (not bound via `local x = require(...)`)
        if tok.is_kw("require") && tokens.get(i + 1).is_some_and(|t| t.is_punct("(")) {
            let already_recorded = require_edges
                .last()
                .is_some_and(|e| e.caller_line == tok.line && e.caller_col == tok.col);
            if !already_recorded {
                if let Some((target_segs, _)) = resolve_dm_expr(&tokens, i + 2, &var_paths, &consts) {
                    let target_service = target_segs.join(".");
                    let target_file = display_path_for(&target_service, service_to_file);
                    require_edges.push(RequireEdge {
                        caller_path: display_path.clone(),
                        caller_line: tok.line,
                        caller_col: tok.col,
                        target_path: target_file,
                        target_service_path: target_service,
                        binding: None,
                    });
                }
            }
        }

        i += 1;
    }

    // Extract exports from `return { ... }` if present
    let mut exports = ScriptExports {
        exported_types,
        ..Default::default()
    };
    if let Some((open, close)) = return_table_span {
        parse_table_literal_exports(
            &tokens,
            open,
            close,
            &local_funcs,
            &display_path,
            service_path,
            &mut exports.exports,
            &mut local_symbols,
        );
    } else if let Some(ret_id) = returned_ident {
        if let Some(members) = table_members.remove(&ret_id) {
            exports.exports = members;
        } else if let Some(func) = local_funcs.get(&ret_id) {
            exports.single_func = Some(func.clone());
        }
    }

    // Third pass: extract RemoteEvent/RemoteFunction callers and handlers
    extract_remote_refs(
        &tokens,
        &display_path,
        &var_paths,
        &consts,
        &created_remotes,
        &local_funcs,
        &local_var_types,
        &mut remotes,
    );

    Some(ScriptData {
        service_path: service_path.to_string(),
        display_path,
        strict,
        tokens,
        consts,
        var_paths,
        require_bindings,
        local_funcs,
        local_var_types,
        created_remotes,
        exports,
        local_symbols,
        require_edges,
        remotes,
    })
}

#[allow(clippy::too_many_arguments)]
fn parse_table_literal_exports(
    tokens: &[Token],
    open_brace: usize,
    close_brace: usize,
    local_funcs: &HashMap<String, FuncSig>,
    display_path: &str,
    service_path: &str,
    out_exports: &mut HashMap<String, ModuleExportKind>,
    out_symbols: &mut Vec<LuauSymbol>,
) {
    let mut i = open_brace + 1;
    let mut brace_depth = 1usize;
    let mut paren_depth = 0usize;
    while i < close_brace {
        let t = &tokens[i];
        if t.is_punct("{") {
            brace_depth += 1;
        } else if t.is_punct("}") {
            brace_depth = brace_depth.saturating_sub(1);
        } else if t.is_punct("(") {
            paren_depth += 1;
        } else if t.is_punct(")") {
            paren_depth = paren_depth.saturating_sub(1);
        } else if brace_depth == 1
            && paren_depth == 0
            && !t.is_member
            && t.ident().is_some()
            && tokens.get(i + 1).is_some_and(|n| n.is_punct("="))
        {
            let key = t.ident().unwrap().to_string();
            let rhs_idx = i + 2;
            if let Some(rhs) = tokens.get(rhs_idx) {
                if rhs.is_kw("function") && tokens.get(rhs_idx + 1).is_some_and(|p| p.is_punct("(")) {
                    if let Some((sig, _)) = parse_func_sig(tokens, rhs_idx + 1, false) {
                        out_symbols.push(LuauSymbol {
                            name: key.clone(),
                            kind: "export".to_string(),
                            path: display_path.to_string(),
                            service_path: service_path.to_string(),
                            line: t.line,
                            col: t.col,
                            signature: Some(sig.signature_str.clone()),
                        });
                        out_exports.insert(key, ModuleExportKind::Func(sig));
                    }
                } else {
                    let kind = rhs
                        .ident()
                        .filter(|_| !rhs.is_member)
                        .and_then(|id| local_funcs.get(id).cloned())
                        .map(ModuleExportKind::Func)
                        .unwrap_or_else(|| ModuleExportKind::Value {
                            signature: None,
                        });
                    let sig = match &kind {
                        ModuleExportKind::Func(f) => Some(f.signature_str.clone()),
                        ModuleExportKind::Value { signature, .. } => signature.clone(),
                    };
                    out_symbols.push(LuauSymbol {
                        name: key.clone(),
                        kind: "export".to_string(),
                        path: display_path.to_string(),
                        service_path: service_path.to_string(),
                        line: t.line,
                        col: t.col,
                        signature: sig,
                    });
                    out_exports.insert(key, kind);
                }
            }
        }
        i += 1;
    }
}

/// Resolve the remote name from the receiver tokens immediately preceding `:` or `.` at `sep_idx`.
fn resolve_remote_receiver_name(
    tokens: &[Token],
    sep_idx: usize,
    var_paths: &HashMap<String, Vec<String>>,
    consts: &HashMap<String, String>,
    created_remotes: &HashMap<String, (String, usize, usize, Option<String>)>,
) -> Option<String> {
    if sep_idx == 0 {
        return None;
    }
    let prev = &tokens[sep_idx - 1];
    if let Some(id) = prev.ident() {
        if !prev.is_member {
            if let Some((_, _, _, Some(rname))) = created_remotes.get(id) {
                return Some(rname.clone());
            }
            if let Some(segs) = var_paths.get(id) {
                return segs.last().cloned();
            }
        } else {
            // `Folder.BuyItem:FireServer(...)`
            return Some(id.to_string());
        }
    }
    if prev.is_punct(")") {
        // E.g. `ReplicatedStorage:WaitForChild("BuyItem"):FireServer(...)`
        let mut depth = 1usize;
        let mut p = sep_idx - 1;
        while p > 0 {
            p -= 1;
            if tokens[p].is_punct(")") {
                depth += 1;
            } else if tokens[p].is_punct("(") {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
        }
        if p >= 2
            && tokens[p - 2].is_punct(":")
            && matches!(tokens[p - 1].ident(), Some("WaitForChild") | Some("FindFirstChild"))
        {
            if let Some(arg) = tokens.get(p + 1) {
                if let Some(s) = arg.str_lit() {
                    return Some(s.to_string());
                }
                if !arg.is_member {
                    if let Some(id) = arg.ident() {
                        return consts.get(id).cloned();
                    }
                }
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn extract_remote_refs(
    tokens: &[Token],
    display_path: &str,
    var_paths: &HashMap<String, Vec<String>>,
    consts: &HashMap<String, String>,
    created_remotes: &HashMap<String, (String, usize, usize, Option<String>)>,
    local_funcs: &HashMap<String, FuncSig>,
    local_var_types: &HashMap<String, String>,
    out: &mut Vec<RemoteRef>,
) {
    let mut i = 0usize;
    while i < tokens.len() {
        let tok = &tokens[i];
        // A. `remote:FireServer(...)` / `FireClient(player, ...)` / `FireAllClients(...)` / `InvokeServer(...)`
        if tok.is_punct(":")
            && tokens.get(i + 2).is_some_and(|p| p.is_punct("("))
        {
            if let Some(method) = tokens.get(i + 1).and_then(Token::ident) {
                let role_info = match method {
                    "FireServer" => Some(("RemoteEvent", "fire_server", 0usize)),
                    "FireClient" => Some(("RemoteEvent", "fire_client", 1usize)),
                    "FireAllClients" => Some(("RemoteEvent", "fire_all_clients", 0usize)),
                    "InvokeServer" => Some(("RemoteFunction", "invoke_server", 0usize)),
                    _ => None,
                };
                if let Some((rkind, role, skip_args)) = role_info {
                    if let Some(rname) =
                        resolve_remote_receiver_name(tokens, i, var_paths, consts, created_remotes)
                    {
                        if let Some((args, _)) = split_call_args(tokens, i + 2) {
                            let payload: Vec<Option<String>> = args
                                .iter()
                                .skip(skip_args)
                                .map(|&(s, e)| infer_expr_type(&tokens[s..e], local_var_types))
                                .collect();
                            let count = payload.len();
                            out.push(RemoteRef {
                                name: rname,
                                remote_kind: rkind.to_string(),
                                role: role.to_string(),
                                path: display_path.to_string(),
                                line: tokens[i + 1].line,
                                col: tokens[i + 1].col,
                                payload_arity_min: count,
                                payload_arity_max: Some(count),
                                payload_types: payload,
                            });
                        }
                    }
                }
            }
        }

        // B. `remote.OnServerEvent:Connect(fn)` / `remote.OnClientEvent:Connect(fn)`
        if tok.is_punct(".")
            && tokens.get(i + 1).is_some_and(|m| matches!(m.ident(), Some("OnServerEvent") | Some("OnClientEvent")))
            && tokens.get(i + 2).is_some_and(|p| p.is_punct(":"))
            && tokens.get(i + 3).is_some_and(|m| matches!(m.ident(), Some("Connect") | Some("Once")))
            && tokens.get(i + 4).is_some_and(|p| p.is_punct("("))
        {
            let event_name = tokens[i + 1].ident().unwrap();
            let (role, skip_params) = if event_name == "OnServerEvent" {
                ("on_server_event", 1usize)
            } else {
                ("on_client_event", 0usize)
            };
            if let Some(rname) =
                resolve_remote_receiver_name(tokens, i, var_paths, consts, created_remotes)
            {
                let sig_opt = if tokens.get(i + 5).is_some_and(|t| t.is_kw("function"))
                    && tokens.get(i + 6).is_some_and(|t| t.is_punct("("))
                {
                    parse_func_sig(tokens, i + 6, false)
                        .map(|(s, _)| s)
                } else if let Some(fn_name) = tokens.get(i + 5).and_then(Token::ident) {
                    local_funcs.get(fn_name).cloned()
                } else {
                    None
                };
                if let Some(sig) = sig_opt {
                    let payload_params: Vec<&ParamSpec> =
                        sig.params.iter().skip(skip_params).collect();
                    let has_vararg = payload_params.iter().any(|p| p.is_vararg);
                    let mut min_a = 0usize;
                    let mut saw_opt = false;
                    let mut fixed = 0usize;
                    let mut ptypes = Vec::new();
                    for p in &payload_params {
                        if p.is_vararg {
                            continue;
                        }
                        fixed += 1;
                        if p.optional || saw_opt {
                            saw_opt = true;
                        } else {
                            min_a += 1;
                        }
                        ptypes.push(p.base_type.clone());
                    }
                    out.push(RemoteRef {
                        name: rname,
                        remote_kind: "RemoteEvent".to_string(),
                        role: role.to_string(),
                        path: display_path.to_string(),
                        line: tokens[i + 1].line,
                        col: tokens[i + 1].col,
                        payload_arity_min: min_a,
                        payload_arity_max: if has_vararg { None } else { Some(fixed) },
                        payload_types: ptypes,
                    });
                }
            }
        }

        // C. `rf.OnServerInvoke = function(player, ...)`
        if tok.is_punct(".")
            && tokens.get(i + 1).and_then(Token::ident) == Some("OnServerInvoke")
            && tokens.get(i + 2).is_some_and(|p| p.is_punct("="))
        {
            if let Some(rname) =
                resolve_remote_receiver_name(tokens, i, var_paths, consts, created_remotes)
            {
                let sig_opt = if tokens.get(i + 3).is_some_and(|t| t.is_kw("function"))
                    && tokens.get(i + 4).is_some_and(|t| t.is_punct("("))
                {
                    parse_func_sig(tokens, i + 4, false)
                        .map(|(s, _)| s)
                } else if let Some(fn_name) = tokens.get(i + 3).and_then(Token::ident) {
                    local_funcs.get(fn_name).cloned()
                } else {
                    None
                };
                if let Some(sig) = sig_opt {
                    let payload_params: Vec<&ParamSpec> = sig.params.iter().skip(1).collect();
                    let has_vararg = payload_params.iter().any(|p| p.is_vararg);
                    let mut min_a = 0usize;
                    let mut saw_opt = false;
                    let mut fixed = 0usize;
                    let mut ptypes = Vec::new();
                    for p in &payload_params {
                        if p.is_vararg {
                            continue;
                        }
                        fixed += 1;
                        if p.optional || saw_opt {
                            saw_opt = true;
                        } else {
                            min_a += 1;
                        }
                        ptypes.push(p.base_type.clone());
                    }
                    out.push(RemoteRef {
                        name: rname,
                        remote_kind: "RemoteFunction".to_string(),
                        role: "on_server_invoke".to_string(),
                        path: display_path.to_string(),
                        line: tokens[i + 1].line,
                        col: tokens[i + 1].col,
                        payload_arity_min: min_a,
                        payload_arity_max: if has_vararg { None } else { Some(fixed) },
                        payload_types: ptypes,
                    });
                }
            }
        }

        i += 1;
    }
}

fn check_module_contracts_for_script(
    script: &ScriptData,
    by_service: &HashMap<String, &ScriptData>,
    mismatches: &mut Vec<LuauContractMismatch>,
) {
    let tokens = &script.tokens;
    let mut seen: HashSet<(usize, String, String)> = HashSet::new();

    // 1. Check `require()` edges whose target module does not exist in Place
    for edge in &script.require_edges {
        if !by_service.contains_key(&edge.target_service_path) {
            let key = (edge.caller_line, edge.target_path.clone(), "missing_export".to_string());
            if seen.insert(key) {
                mismatches.push(LuauContractMismatch {
                    caller_path: script.display_path.clone(),
                    caller_line: edge.caller_line,
                    target_path: edge.target_path.clone(),
                    kind: "missing_export".to_string(),
                    message: format!(
                        "{}:{}: required module '{}' does not exist in Place",
                        script.display_path, edge.caller_line, edge.target_service_path
                    ),
                });
            }
        }
    }

    // 2. Check `Mod.Member` / `Mod:Member` usages against target module's exports
    let mut i = 0usize;
    let mut in_type_decl = false;
    let mut type_decl_line = 0usize;
    while i < tokens.len() {
        let tok = &tokens[i];
        if in_type_decl && tok.line != type_decl_line {
            in_type_decl = false;
        }
        if tok.is_kw("type") {
            in_type_decl = true;
            type_decl_line = tok.line;
        }
        // Skip type annotations `: Type` / `:: Type`
        if (tok.is_punct(":") || tok.is_punct("::"))
            && !(tok.is_punct(":")
                && tokens.get(i + 1).and_then(Token::ident).is_some()
                && tokens.get(i + 2).is_some_and(|p| p.is_punct("(")))
        {
            i = skip_type_expr(tokens, i + 1);
            continue;
        }

        if !in_type_decl && !tok.is_member {
            if let Some(ident) = tok.ident() {
                if let Some(target_service) = script.require_bindings.get(ident) {
                    if let Some(target) = by_service.get(target_service) {
                        if tokens.get(i + 1).is_some_and(|s| s.is_punct(".") || s.is_punct(":")) {
                            if let Some(member_tok) = tokens.get(i + 2) {
                                if let Some(member_name) = member_tok.ident() {
                                    let is_colon_call = tokens[i + 1].is_punct(":");
                                    let is_call = tokens.get(i + 3).is_some_and(|p| p.is_punct("("));
                                    match target.exports.exports.get(member_name) {
                                        None => {
                                            if !target.exports.exported_types.contains(member_name) {
                                                let key = (
                                                    member_tok.line,
                                                    target.display_path.clone(),
                                                    format!("missing_export:{member_name}"),
                                                );
                                                if seen.insert(key) {
                                                    mismatches.push(LuauContractMismatch {
                                                        caller_path: script.display_path.clone(),
                                                        caller_line: member_tok.line,
                                                        target_path: target.display_path.clone(),
                                                        kind: "missing_export".to_string(),
                                                        message: format!(
                                                            "{}:{}: '{}' is not exported by {}",
                                                            script.display_path,
                                                            member_tok.line,
                                                            member_name,
                                                            target.display_path
                                                        ),
                                                    });
                                                }
                                            }
                                        }
                                        Some(ModuleExportKind::Func(sig)) if is_call => {
                                            if let Some((args, _)) = split_call_args(tokens, i + 3) {
                                                let call_argc = args.len();
                                                let effective_argc = if sig.is_method && !is_colon_call {
                                                    if call_argc == 0 {
                                                        // Missing `self` + required args
                                                        usize::MAX
                                                    } else {
                                                        call_argc - 1
                                                    }
                                                } else if !sig.is_method && is_colon_call {
                                                    call_argc + 1
                                                } else {
                                                    call_argc
                                                };
                                                let arity_bad = effective_argc == usize::MAX
                                                    || effective_argc < sig.min_arity
                                                    || sig.max_arity.is_some_and(|max| effective_argc > max);
                                                if arity_bad {
                                                    let expected_desc = match sig.max_arity {
                                                        Some(max) if max == sig.min_arity => format!("{max}"),
                                                        Some(max) => format!("{}..{max}", sig.min_arity),
                                                        None => format!("{}+", sig.min_arity),
                                                    };
                                                    let shown_argc = if effective_argc == usize::MAX { 0 } else { effective_argc };
                                                    mismatches.push(LuauContractMismatch {
                                                        caller_path: script.display_path.clone(),
                                                        caller_line: member_tok.line,
                                                        target_path: target.display_path.clone(),
                                                        kind: "arity_mismatch".to_string(),
                                                        message: format!(
                                                            "{}:{}: call to '{ident}.{member_name}' passed {shown_argc} arg(s), expected {expected_desc} ({})",
                                                            script.display_path,
                                                            member_tok.line,
                                                            sig.signature_str
                                                        ),
                                                    });
                                                } else {
                                                    // Check argument types against parameter types
                                                    let skip_arg = if sig.is_method && !is_colon_call { 1 } else { 0 };
                                                    let skip_param = if !sig.is_method && is_colon_call { 1 } else { 0 };
                                                    for (arg_idx, &(a_start, a_end)) in args.iter().skip(skip_arg).enumerate() {
                                                        let Some(param) = sig.params.get(arg_idx + skip_param) else {
                                                            break;
                                                        };
                                                        if param.is_vararg {
                                                            break;
                                                        }
                                                        let actual = infer_expr_type(&tokens[a_start..a_end], &script.local_var_types);
                                                        if !types_compatible(param.base_type.as_deref(), param.optional, actual.as_deref()) {
                                                            mismatches.push(LuauContractMismatch {
                                                                caller_path: script.display_path.clone(),
                                                                caller_line: member_tok.line,
                                                                target_path: target.display_path.clone(),
                                                                kind: "strict_type_error".to_string(),
                                                                message: format!(
                                                                    "{}:{}: argument '{}' to '{ident}.{member_name}' has type '{}', expected '{}'",
                                                                    script.display_path,
                                                                    member_tok.line,
                                                                    param.name,
                                                                    actual.as_deref().unwrap_or("unknown"),
                                                                    param.raw_type.as_deref().unwrap_or("unknown"),
                                                                ),
                                                            });
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }
}

fn compute_function_end_map(tokens: &[Token]) -> HashMap<usize, usize> {
    let mut blocks: Vec<(bool, usize)> = Vec::new();
    let mut map = HashMap::new();
    for (idx, tok) in tokens.iter().enumerate() {
        if tok.is_member {
            continue;
        }
        let Some(word) = tok.ident() else { continue };
        match word {
            "function" => blocks.push((true, idx)),
            "if" => {
                let prev = if idx > 0 { Some(&tokens[idx - 1]) } else { None };
                if !is_if_expression_context(prev) {
                    blocks.push((false, idx));
                }
            }
            "for" | "while" | "repeat" => blocks.push((false, idx)),
            "do" => {
                let is_loop_do = idx > 0 && {
                    let mut back = idx;
                    let mut found = false;
                    while back > 0 {
                        back -= 1;
                        if tokens[back].is_kw("for") || tokens[back].is_kw("while") {
                            found = true;
                            break;
                        }
                        if tokens[back].is_kw("do")
                            || tokens[back].is_kw("then")
                            || tokens[back].is_kw("end")
                            || tokens[back].is_kw("function")
                        {
                            break;
                        }
                    }
                    found
                };
                if !is_loop_do {
                    blocks.push((false, idx));
                }
            }
            "end" | "until" => {
                if let Some((is_fn, open_idx)) = blocks.pop() {
                    if is_fn && word == "end" {
                        map.insert(open_idx, idx);
                    }
                }
            }
            _ => {}
        }
    }
    map
}

fn check_strict_builtin_for_script(script: &ScriptData, mismatches: &mut Vec<LuauContractMismatch>) {
    if !script.strict {
        return;
    }
    let tokens = &script.tokens;
    let func_ends = compute_function_end_map(tokens);
    let mut i = 0usize;
    while i < tokens.len() {
        let tok = &tokens[i];
        // 1. Annotated local assignment mismatch: `local x: number = "bad"`
        if tok.is_kw("local")
            && tokens.get(i + 1).is_some_and(|t| !t.is_member && t.ident().is_some())
            && !tokens.get(i + 1).is_some_and(|t| t.is_kw("function"))
            && tokens.get(i + 2).is_some_and(|t| t.is_punct(":"))
        {
            let ty_start = i + 3;
            let ty_end = skip_type_expr(tokens, ty_start);
            if ty_end > ty_start && tokens.get(ty_end).is_some_and(|t| t.is_punct("=")) {
                let raw_ty = render_tokens_compact(&tokens[ty_start..ty_end]);
                let (exp_base, exp_opt) = normalize_type_name(&raw_ty);
                if exp_base.is_some() {
                    let rhs_start = ty_end + 1;
                    // Inspect simple single-token / unary / table literal RHS
                    let rhs_end = find_simple_expr_end(tokens, rhs_start);
                    if rhs_end > rhs_start {
                        let actual = infer_expr_type(&tokens[rhs_start..rhs_end], &script.local_var_types);
                        if !types_compatible(exp_base.as_deref(), exp_opt, actual.as_deref()) {
                            mismatches.push(LuauContractMismatch {
                                caller_path: script.display_path.clone(),
                                caller_line: tok.line,
                                target_path: script.display_path.clone(),
                                kind: "strict_type_error".to_string(),
                                message: format!(
                                    "{}:{}: cannot assign '{}' to '{}' (expected '{raw_ty}')",
                                    script.display_path,
                                    tok.line,
                                    actual.as_deref().unwrap_or("unknown"),
                                    tokens[i + 1].ident().unwrap_or("local")
                                ),
                            });
                        }
                    }
                }
            }
        }

        // 2. Local typed function call mismatch: `helper("bad", 1)`
        if !tok.is_member
            && tokens.get(i + 1).is_some_and(|p| p.is_punct("("))
            && !(i > 0 && (tokens[i - 1].is_kw("function") || tokens[i - 1].is_punct(".") || tokens[i - 1].is_punct(":")))
        {
            if let Some(fn_name) = tok.ident() {
                if let Some(sig) = script.local_funcs.get(fn_name) {
                    if let Some((args, _)) = split_call_args(tokens, i + 1) {
                        if args.len() < sig.min_arity || sig.max_arity.is_some_and(|m| args.len() > m) {
                            mismatches.push(LuauContractMismatch {
                                caller_path: script.display_path.clone(),
                                caller_line: tok.line,
                                target_path: script.display_path.clone(),
                                kind: "arity_mismatch".to_string(),
                                message: format!(
                                    "{}:{}: call to '{fn_name}' passed {} arg(s), expected {}",
                                    script.display_path,
                                    tok.line,
                                    args.len(),
                                    sig.signature_str
                                ),
                            });
                        } else {
                            for (a_idx, &(s, e)) in args.iter().enumerate() {
                                let Some(param) = sig.params.get(a_idx) else { break };
                                if param.is_vararg { break; }
                                let actual = infer_expr_type(&tokens[s..e], &script.local_var_types);
                                if !types_compatible(param.base_type.as_deref(), param.optional, actual.as_deref()) {
                                    mismatches.push(LuauContractMismatch {
                                        caller_path: script.display_path.clone(),
                                        caller_line: tok.line,
                                        target_path: script.display_path.clone(),
                                        kind: "strict_type_error".to_string(),
                                        message: format!(
                                            "{}:{}: argument '{}' to '{fn_name}' has type '{}', expected '{}'",
                                            script.display_path,
                                            tok.line,
                                            param.name,
                                            actual.as_deref().unwrap_or("unknown"),
                                            param.raw_type.as_deref().unwrap_or("unknown"),
                                        ),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // 3. Return type mismatch inside `[local] function ...(): RetType`
        if tok.is_kw("function") {
            let open_p = if tokens.get(i + 1).is_some_and(|p| p.is_punct("(")) {
                Some(i + 1)
            } else if tokens.get(i + 2).is_some_and(|p| p.is_punct("(")) {
                Some(i + 2)
            } else if tokens.get(i + 4).is_some_and(|p| p.is_punct("(")) {
                Some(i + 4)
            } else {
                None
            };
            if let Some(op) = open_p {
                if let (Some((sig, body_start)), Some(&end_idx)) = (
                    parse_func_sig(tokens, op, false),
                    func_ends.get(&i),
                ) {
                    if let (Some(ret_base), Some(ret_raw)) =
                        (sig.return_base_type.as_deref(), sig.return_type.as_deref())
                    {
                        let (_, ret_opt) = normalize_type_name(ret_raw);
                        check_function_returns(
                            tokens,
                            body_start,
                            end_idx,
                            ret_base,
                            ret_opt,
                            ret_raw,
                            &script.display_path,
                            &script.local_var_types,
                            &func_ends,
                            mismatches,
                        );
                    }
                }
            }
        }
        i += 1;
    }
}

fn find_simple_expr_end(tokens: &[Token], start: usize) -> usize {
    let Some(first) = tokens.get(start) else {
        return start;
    };
    if first.is_punct("{") {
        if let Some(c) = find_matching_brace(tokens, start) {
            return c + 1;
        }
    }
    if first.is_punct("-") && tokens.get(start + 1).is_some_and(|t| matches!(t.kind, TokenKind::Number(_))) {
        return if tokens.get(start + 2).is_some_and(|t| t.line == first.line && t.is_punct("..")) {
            start
        } else {
            start + 2
        };
    }
    let single = matches!(
        first.kind,
        TokenKind::Number(_) | TokenKind::StringLit(_)
    ) || first.is_kw("true")
        || first.is_kw("false")
        || first.is_kw("nil");
    if single {
        // Only treat as literal expression if not immediately followed by binary/member operators on same line
        if let Some(next) = tokens.get(start + 1) {
            if next.line == first.line
                && (next.is_punct("..")
                    || next.is_punct("+")
                    || next.is_punct("-")
                    || next.is_punct("*")
                    || next.is_punct("/")
                    || next.is_punct("or")
                    || next.is_punct("and")
                    || next.is_kw("or")
                    || next.is_kw("and")
                    || next.is_punct("::"))
            {
                return start;
            }
        }
        return start + 1;
    }
    start
}

#[allow(clippy::too_many_arguments)]
fn check_function_returns(
    tokens: &[Token],
    body_start: usize,
    body_end: usize,
    ret_base: &str,
    ret_opt: bool,
    ret_raw: &str,
    display_path: &str,
    local_var_types: &HashMap<String, String>,
    func_ends: &HashMap<usize, usize>,
    mismatches: &mut Vec<LuauContractMismatch>,
) {
    let mut k = body_start;
    while k < body_end {
        let t = &tokens[k];
        if !t.is_member {
            if t.is_kw("function") {
                if let Some(&nested_end) = func_ends.get(&k) {
                    k = nested_end + 1;
                    continue;
                }
                break;
            } else if t.is_kw("return") {
                let expr_start = k + 1;
                if expr_start < body_end && tokens[expr_start].line == t.line {
                    let expr_end = find_simple_expr_end(tokens, expr_start);
                    if expr_end > expr_start {
                        let actual = infer_expr_type(&tokens[expr_start..expr_end], local_var_types);
                        if !types_compatible(Some(ret_base), ret_opt, actual.as_deref()) {
                            mismatches.push(LuauContractMismatch {
                                caller_path: display_path.to_string(),
                                caller_line: t.line,
                                target_path: display_path.to_string(),
                                kind: "strict_type_error".to_string(),
                                message: format!(
                                    "{display_path}:{}: return type '{}' does not match declared return type '{ret_raw}'",
                                    t.line,
                                    actual.as_deref().unwrap_or("unknown")
                                ),
                            });
                        }
                    }
                }
            }
        }
        k += 1;
    }
}

fn check_remote_contracts(remotes: &[RemoteRef], mismatches: &mut Vec<LuauContractMismatch>) {
    let mut by_name: HashMap<&str, Vec<&RemoteRef>> = HashMap::new();
    for r in remotes {
        by_name.entry(r.name.as_str()).or_default().push(r);
    }

    for (name, refs) in by_name {
        let pairs = [
            ("fire_server", "on_server_event"),
            ("fire_client", "on_client_event"),
            ("fire_all_clients", "on_client_event"),
            ("invoke_server", "on_server_invoke"),
        ];
        for (caller_role, handler_role) in pairs {
            let callers: Vec<&&RemoteRef> = refs.iter().filter(|r| r.role == caller_role).collect();
            let handlers: Vec<&&RemoteRef> = refs.iter().filter(|r| r.role == handler_role).collect();
            for caller in &callers {
                for handler in &handlers {
                    let argc = caller.payload_arity_min;
                    let arity_bad = argc < handler.payload_arity_min
                        || handler.payload_arity_max.is_some_and(|m| argc > m);
                    if arity_bad {
                        let exp = match handler.payload_arity_max {
                            Some(m) if m == handler.payload_arity_min => format!("{m}"),
                            Some(m) => format!("{}..{m}", handler.payload_arity_min),
                            None => format!("{}+", handler.payload_arity_min),
                        };
                        mismatches.push(LuauContractMismatch {
                            caller_path: caller.path.clone(),
                            caller_line: caller.line,
                            target_path: handler.path.clone(),
                            kind: "remote_arg_mismatch".to_string(),
                            message: format!(
                                "{}:{}: '{name}' {caller_role} sends {argc} payload arg(s), but {handler_role} at {}:{} expects {exp}",
                                caller.path, caller.line, handler.path, handler.line
                            ),
                        });
                        continue;
                    }
                    for (idx, exp_ty) in handler.payload_types.iter().enumerate() {
                        let Some(expected) = exp_ty.as_deref() else { continue };
                        let Some(actual) = caller.payload_types.get(idx).and_then(|o| o.as_deref()) else {
                            continue;
                        };
                        if !types_compatible(Some(expected), false, Some(actual)) {
                            mismatches.push(LuauContractMismatch {
                                caller_path: caller.path.clone(),
                                caller_line: caller.line,
                                target_path: handler.path.clone(),
                                kind: "remote_arg_mismatch".to_string(),
                                message: format!(
                                    "{}:{}: '{name}' {caller_role} payload arg #{} has type '{actual}', but {handler_role} at {}:{} expects '{expected}'",
                                    caller.path,
                                    caller.line,
                                    idx + 1,
                                    handler.path,
                                    handler.line
                                ),
                            });
                        }
                    }
                }
            }
        }
    }
}

fn detect_require_cycles(
    require_edges: &[RequireEdge],
    mismatches: &mut Vec<LuauContractMismatch>,
) {
    let mut adj: BTreeMap<&str, Vec<&RequireEdge>> = BTreeMap::new();
    for edge in require_edges {
        adj.entry(edge.caller_path.as_str()).or_default().push(edge);
    }
    // 0 = unvisited, 1 = on active DFS stack, 2 = completed
    let mut state: HashMap<&str, u8> = HashMap::new();
    let mut reported: HashSet<(String, String)> = HashSet::new();

    for &start in adj.keys() {
        if state.get(start).copied().unwrap_or(0) != 0 {
            continue;
        }
        let mut stack: Vec<(&str, usize)> = vec![(start, 0)];
        let mut path_stack: Vec<&str> = vec![start];
        state.insert(start, 1);

        while let Some((node, edge_idx)) = stack.pop() {
            let edges = adj.get(node).map(Vec::as_slice).unwrap_or(&[]);
            if edge_idx < edges.len() {
                stack.push((node, edge_idx + 1));
                let edge = edges[edge_idx];
                let target = edge.target_path.as_str();
                match state.get(target).copied().unwrap_or(0) {
                    1 => {
                        if reported.insert((edge.caller_path.clone(), edge.target_path.clone())) {
                            let cycle_start = path_stack
                                .iter()
                                .position(|&p| p == target)
                                .unwrap_or(0);
                            let mut chain: Vec<&str> = path_stack[cycle_start..].to_vec();
                            chain.push(target);
                            mismatches.push(LuauContractMismatch {
                                caller_path: edge.caller_path.clone(),
                                caller_line: edge.caller_line,
                                target_path: edge.target_path.clone(),
                                kind: "cyclic_require".to_string(),
                                message: format!(
                                    "{}:{}: cyclic require() detected ({})",
                                    edge.caller_path,
                                    edge.caller_line,
                                    chain.join(" -> ")
                                ),
                            });
                        }
                    }
                    0 => {
                        state.insert(target, 1);
                        path_stack.push(target);
                        stack.push((target, 0));
                    }
                    _ => {}
                }
            } else {
                state.insert(node, 2);
                path_stack.pop();
            }
        }
    }
}

pub fn find_luau_analyze() -> Option<std::path::PathBuf> {
    crate::projects::find_tool("luau-analyze", "OPENRO_LUAU_ANALYZE")
}

fn parse_luau_analyze_line(line: &str, dir: &Path) -> Option<WorkspaceDiagnostic> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Format 1: `path(line,col): Kind: message` or `path(line,col-end): Kind: message`
    if let Some(open_p) = trimmed.find('(') {
        if let Some(close_rel) = trimmed[open_p + 1..].find("):") {
            let file_raw = trimmed[..open_p].trim();
            let coords = &trimmed[open_p + 1..open_p + 1 + close_rel];
            let rest = trimmed[open_p + 1 + close_rel + 2..].trim();
            if let Some((l_str, c_str)) = coords.split_once(',') {
                if let Ok(line_no) = l_str.trim().parse::<usize>() {
                    let col_no = c_str
                        .split('-')
                        .next()
                        .and_then(|s| s.trim().parse::<usize>().ok());
                    let (severity, msg) = classify_luau_analyze_msg(rest);
                    let rel = normalize_diag_path(file_raw, dir)?;
                    return Some(WorkspaceDiagnostic {
                        path: rel,
                        service_path: None,
                        line: line_no,
                        col: col_no,
                        severity,
                        code: "LUAU_ANALYZE".to_string(),
                        source: "luau-analyze".to_string(),
                        message: msg,
                    });
                }
            }
        }
    }
    // Format 2: `path:line:col: Kind: message`
    let mut parts = trimmed.splitn(4, ':');
    let p_str = parts.next()?.trim();
    let l_str = parts.next()?.trim();
    let c_str = parts.next()?.trim();
    let rest = parts.next()?.trim();
    if let (Ok(line_no), Ok(col_no)) = (l_str.parse::<usize>(), c_str.parse::<usize>()) {
        let (severity, msg) = classify_luau_analyze_msg(rest);
        let rel = normalize_diag_path(p_str, dir)?;
        return Some(WorkspaceDiagnostic {
            path: rel,
            service_path: None,
            line: line_no,
            col: Some(col_no),
            severity,
            code: "LUAU_ANALYZE".to_string(),
            source: "luau-analyze".to_string(),
            message: msg,
        });
    }
    None
}

fn classify_luau_analyze_msg(rest: &str) -> (Severity, String) {
    let lower = rest.to_ascii_lowercase();
    let severity = if lower.starts_with("warning:") {
        Severity::Warning
    } else if lower.starts_with("info:") {
        Severity::Info
    } else {
        Severity::Error
    };
    (severity, rest.to_string())
}

fn normalize_diag_path(raw: &str, dir: &Path) -> Option<String> {
    let cleaned = raw.trim();
    if cleaned.is_empty() || cleaned.contains('\0') || cleaned.chars().any(|c| c.is_control()) {
        return None;
    }
    let p = std::path::Path::new(cleaned);
    if p.is_absolute() {
        if let Ok(rel) = p.strip_prefix(dir) {
            return normalize_luau_rel_path(&rel.to_string_lossy().replace('\\', "/"));
        }
        if let (Ok(canon_dir), Ok(canon_p)) = (dir.canonicalize(), p.canonicalize()) {
            if let Ok(rel) = canon_p.strip_prefix(&canon_dir) {
                return normalize_luau_rel_path(&rel.to_string_lossy().replace('\\', "/"));
            }
        }
        return None;
    }
    let slash = cleaned.replace('\\', "/");
    normalize_luau_rel_path(&slash)
}

fn run_luau_analyze_bin(
    bin: &std::path::Path,
    dir: &Path,
    service_to_file: &BTreeMap<String, String>,
) -> Vec<WorkspaceDiagnostic> {
    let mut files: Vec<String> = service_to_file
        .values()
        .filter(|f| {
            (f.ends_with(".luau") || f.ends_with(".lua"))
                && !f.starts_with('-')
                && normalize_luau_rel_path(f).is_some()
                && crate::projects::safe_join(dir, f).is_ok()
        })
        .cloned()
        .collect();
    files.sort();
    files.dedup();
    if files.is_empty() {
        return Vec::new();
    }
    let file_to_service: HashMap<&str, &str> = service_to_file
        .iter()
        .map(|(s, f)| (f.as_str(), s.as_str()))
        .collect();
    let Ok((_code, timed_out, _ms, stdout, stderr)) =
        crate::projects::run_bounded(bin, dir, &files, 30)
    else {
        return Vec::new();
    };
    if timed_out {
        return vec![WorkspaceDiagnostic {
            path: String::new(),
            service_path: None,
            line: 1,
            col: None,
            severity: Severity::Warning,
            code: "LUAU_ANALYZE_TIMEOUT".to_string(),
            source: "luau-analyze".to_string(),
            message: "luau-analyze timed out after 30s".to_string(),
        }];
    }
    let mut out = Vec::new();
    for line in stdout.lines().chain(stderr.lines()) {
        if let Some(mut diag) = parse_luau_analyze_line(line, dir) {
            if let Some(sp) = file_to_service.get(diag.path.as_str()) {
                diag.service_path = Some((*sp).to_string());
            }
            out.push(diag);
        }
    }
    out
}

/// Build the full workspace symbol index, cross-file contract analysis, and optional
/// `luau-analyze` diagnostics across a Place.
pub fn analyze_workspace_symbols(
    scripts: &BTreeMap<String, String>,
    instances: &BTreeMap<String, Value>,
    service_to_file: &BTreeMap<String, String>,
    project_dir: Option<&Path>,
    run_luau_analyze: bool,
) -> WorkspaceSymbolIndex {
    let place_report = analyze_place(scripts, instances);

    let mut symbols: Vec<LuauSymbol> = Vec::new();
    let mut require_edges: Vec<RequireEdge> = Vec::new();
    let mut remotes: Vec<RemoteRef> = Vec::new();
    let mut parsed: Vec<ScriptData> = Vec::new();

    // Index static RemoteEvent / RemoteFunction instances declared in Rojo tree
    for (inst_path, info) in instances {
        let class_name = info
            .get("class_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if class_name == "RemoteEvent" || class_name == "RemoteFunction" {
            let name = inst_path.rsplit('.').next().unwrap_or(inst_path).to_string();
            let file_path = info
                .get("source_file")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| display_path_for(inst_path, service_to_file));
            symbols.push(LuauSymbol {
                name,
                kind: if class_name == "RemoteFunction" {
                    "remote_function".to_string()
                } else {
                    "remote_event".to_string()
                },
                path: file_path,
                service_path: inst_path.clone(),
                line: 1,
                col: 1,
                signature: Some(class_name.to_string()),
            });
        }
    }

    for (service_path, source) in scripts {
        if let Some(sd) = extract_script_data(service_path, source, service_to_file) {
            symbols.extend(sd.local_symbols.iter().cloned());
            require_edges.extend(sd.require_edges.iter().cloned());
            remotes.extend(sd.remotes.iter().cloned());
            parsed.push(sd);
        }
    }

    let by_service: HashMap<String, &ScriptData> = parsed
        .iter()
        .map(|sd| (sd.service_path.clone(), sd))
        .collect();

    let mut mismatches: Vec<LuauContractMismatch> = Vec::new();
    for sd in &parsed {
        check_module_contracts_for_script(sd, &by_service, &mut mismatches);
        check_strict_builtin_for_script(sd, &mut mismatches);
    }
    detect_require_cycles(&require_edges, &mut mismatches);
    check_remote_contracts(&remotes, &mut mismatches);

    let mut diagnostics: Vec<WorkspaceDiagnostic> = place_report
        .diagnostics
        .iter()
        .map(|d| WorkspaceDiagnostic {
            path: display_path_for(&d.path, service_to_file),
            service_path: Some(d.path.clone()),
            line: d.line,
            col: None,
            severity: d.severity,
            code: d.code.to_string(),
            source: "builtin".to_string(),
            message: d.message.clone(),
        })
        .collect();

    for m in &mismatches {
        diagnostics.push(WorkspaceDiagnostic {
            path: m.caller_path.clone(),
            service_path: None,
            line: m.caller_line,
            col: None,
            severity: Severity::Error,
            code: m.kind.clone(),
            source: "builtin".to_string(),
            message: m.message.clone(),
        });
    }

    let luau_bin = find_luau_analyze();
    let luau_analyze_available = luau_bin.is_some();
    let mut diagnostics_source = "builtin".to_string();
    if run_luau_analyze {
        if let (Some(bin), Some(dir)) = (luau_bin, project_dir) {
            let ext_diags = run_luau_analyze_bin(&bin, dir, service_to_file);
            for ed in ext_diags {
                if ed.severity == Severity::Error {
                    mismatches.push(LuauContractMismatch {
                        caller_path: ed.path.clone(),
                        caller_line: ed.line,
                        target_path: ed.path.clone(),
                        kind: "strict_type_error".to_string(),
                        message: ed.message.clone(),
                    });
                }
                diagnostics.push(ed);
            }
            diagnostics_source = "luau-analyze+builtin".to_string();
        }
    }

    let ok = !diagnostics.iter().any(|d| d.severity == Severity::Error) && mismatches.is_empty();
    WorkspaceSymbolIndex {
        ok,
        luau_analyze_available,
        diagnostics_source,
        symbols,
        require_edges,
        remotes,
        mismatches,
        diagnostics,
    }
}

/// Resolve definition for a symbol, `require()` target, or Remote reference at `(path, line, col)`.
pub fn lookup_definition(
    scripts: &BTreeMap<String, String>,
    instances: &BTreeMap<String, Value>,
    service_to_file: &BTreeMap<String, String>,
    path: &str,
    line: usize,
    col: usize,
) -> DefinitionResult {
    if line == 0 || col == 0 {
        return DefinitionResult::not_found();
    }
    let Some(norm) = normalize_luau_rel_path(path) else {
        return DefinitionResult::not_found();
    };
    let index = analyze_workspace_symbols(scripts, instances, service_to_file, None, false);

    // Find matching ScriptData by rel path or service path
    let mut matched_service: Option<String> = None;
    for sp in scripts.keys() {
        let dp = display_path_for(sp, service_to_file);
        if dp == norm || *sp == norm {
            matched_service = Some(sp.clone());
            break;
        }
    }
    let Some(caller_service) = matched_service else {
        return DefinitionResult::not_found();
    };
    let Some(source) = scripts.get(&caller_service) else {
        return DefinitionResult::not_found();
    };
    let Some(sd) = extract_script_data(&caller_service, source, service_to_file) else {
        return DefinitionResult::not_found();
    };

    // Pick token on `line` covering `col` (or closest identifier/string on `line`)
    let line_tokens: Vec<(usize, &Token)> = sd
        .tokens
        .iter()
        .enumerate()
        .filter(|(_, t)| t.line == line)
        .collect();
    if line_tokens.is_empty() {
        return DefinitionResult::not_found();
    }

    let picked_pos = line_tokens
        .iter()
        .find(|(_, t)| col >= t.col && col < t.col + t.span_len().max(1))
        .or_else(|| {
            line_tokens
                .iter()
                .filter(|(_, t)| t.ident().is_some() || t.str_lit().is_some())
                .min_by_key(|(_, t)| t.col.abs_diff(col))
        })
        .map(|&(pos, _)| pos);

    let Some(tok_idx) = picked_pos else {
        return DefinitionResult::not_found();
    };
    let tok = &sd.tokens[tok_idx];

    // 1. `Mod.Member` or `Mod:Member` (cursor on Member or Mod)
    if tok.is_member
        && tok_idx >= 2
        && (sd.tokens[tok_idx - 1].is_punct(".") || sd.tokens[tok_idx - 1].is_punct(":"))
    {
        if let Some(member) = tok.ident() {
            if let Some(recv) = sd.tokens[tok_idx - 2].ident() {
                if let Some(target_sp) = sd.require_bindings.get(recv) {
                    if let Some(sym) = index
                        .symbols
                        .iter()
                        .find(|s| s.service_path == *target_sp && s.name == member)
                    {
                        return DefinitionResult {
                            ok: true,
                            target_path: Some(sym.path.clone()),
                            line: Some(sym.line),
                            col: Some(sym.col),
                            symbol: Some(sym.clone()),
                        };
                    }
                }
            }
            // Remote method (`FireServer` / `InvokeServer` / `OnServerEvent`)
            if matches!(
                member,
                "FireServer" | "InvokeServer" | "FireClient" | "FireAllClients" | "OnServerEvent" | "OnClientEvent" | "OnServerInvoke"
            ) {
                if let Some(rname) = resolve_remote_receiver_name(
                    &sd.tokens,
                    tok_idx - 1,
                    &sd.var_paths,
                    &sd.consts,
                    &sd.created_remotes,
                ) {
                    let target_handler = index.remotes.iter().find(|r| {
                        r.name == rname
                            && matches!(
                                r.role.as_str(),
                                "on_server_event" | "on_server_invoke" | "on_client_event"
                            )
                    });
                    if let Some(handler) = target_handler {
                        let sym = index
                            .symbols
                            .iter()
                            .find(|s| {
                                s.name == rname
                                    && matches!(s.kind.as_str(), "remote_event" | "remote_function")
                            })
                            .cloned()
                            .unwrap_or(LuauSymbol {
                                name: rname.clone(),
                                kind: if handler.remote_kind == "RemoteFunction" {
                                    "remote_function".to_string()
                                } else {
                                    "remote_event".to_string()
                                },
                                path: handler.path.clone(),
                                service_path: handler.path.clone(),
                                line: handler.line,
                                col: handler.col,
                                signature: Some(handler.role.clone()),
                            });
                        return DefinitionResult {
                            ok: true,
                            target_path: Some(handler.path.clone()),
                            line: Some(handler.line),
                            col: Some(handler.col),
                            symbol: Some(sym),
                        };
                    }
                }
            }
        }
    }

    // 2. Cursor on a `require(...)` edge line or binding variable
    if let Some(edge) = sd
        .require_edges
        .iter()
        .find(|e| e.caller_line == line || e.binding.as_deref() == tok.ident())
    {
        if scripts.contains_key(&edge.target_service_path) {
            let sym = index
                .symbols
                .iter()
                .find(|s| s.service_path == edge.target_service_path && s.kind == "export")
                .or_else(|| {
                    index
                        .symbols
                        .iter()
                        .find(|s| s.service_path == edge.target_service_path)
                })
                .cloned();
            return DefinitionResult {
                ok: true,
                target_path: Some(edge.target_path.clone()),
                line: Some(1),
                col: Some(1),
                symbol: sym,
            };
        }
    }

    // 3. Cursor on a Remote variable or string name (e.g. `WaitForChild("PurchaseItem")` or `ev`)
    let remote_candidate = tok
        .str_lit()
        .map(String::from)
        .or_else(|| {
            tok.ident().and_then(|id| {
                if let Some((_, _, _, Some(rn))) = sd.created_remotes.get(id) {
                    Some(rn.clone())
                } else {
                    sd.var_paths.get(id).and_then(|segs| segs.last().cloned())
                }
            })
        });
    if let Some(rname) = remote_candidate {
        if let Some(sym) = index.symbols.iter().find(|s| {
            s.name == rname && matches!(s.kind.as_str(), "remote_event" | "remote_function")
        }) {
            return DefinitionResult {
                ok: true,
                target_path: Some(sym.path.clone()),
                line: Some(sym.line),
                col: Some(sym.col),
                symbol: Some(sym.clone()),
            };
        }
    }

    // 4. Local symbol in current script (function, type, export)
    if let Some(name) = tok.ident() {
        if let Some(sym) = sd.local_symbols.iter().find(|s| s.name == name) {
            return DefinitionResult {
                ok: true,
                target_path: Some(sym.path.clone()),
                line: Some(sym.line),
                col: Some(sym.col),
                symbol: Some(sym.clone()),
            };
        }
    }

    DefinitionResult::not_found()
}
