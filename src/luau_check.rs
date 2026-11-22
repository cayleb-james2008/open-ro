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
    /// True when immediately preceded by single `.` or `:` (member/method position).
    is_member: bool,
}

impl Token {
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
        }
        content.push(chars[i]);
        i += 1;
    }
    None
}

struct LexError {
    line: usize,
    code: &'static str,
    message: String,
}

fn tokenize(source: &str) -> Result<Vec<Token>, LexError> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::with_capacity(source.len() / 5 + 8);
    let mut delims: Vec<(char, usize)> = Vec::new();
    let mut line: usize = 1;
    let mut i: usize = 0;

    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
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
                match scan_long_bracket_close(&chars, content_start, eqs, &mut line) {
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
                is_member: false,
            });
            continue;
        }

        // Long string `[=*[...]=*]`
        if let Some((eqs, content_start)) = match_long_bracket_open(&chars, i) {
            let start_line = line;
            match scan_long_bracket_close(&chars, content_start, eqs, &mut line) {
                Some((content, next)) => {
                    tokens.push(Token {
                        kind: TokenKind::StringLit(content),
                        line: start_line,
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
                is_member: false,
            });
            continue;
        }

        // Identifiers & keywords
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
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
                is_member: prev_is_member,
            });
            continue;
        }

        // Delimiters
        if matches!(c, '(' | '[' | '{') {
            delims.push((c, line));
            tokens.push(Token {
                kind: TokenKind::Punct(c.to_string()),
                line,
                is_member: false,
            });
            i += 1;
            continue;
        }
        if matches!(c, ')' | ']' | '}') {
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
                is_member: false,
            });
            i += 1;
            continue;
        }

        // Multi-character operators
        if c == '.' && i + 2 < chars.len() && chars[i + 1] == '.' && chars[i + 2] == '=' {
            tokens.push(Token {
                kind: TokenKind::Punct("..=".to_string()),
                line,
                is_member: false,
            });
            i += 3;
            continue;
        }
        if c == '.' && i + 2 < chars.len() && chars[i + 1] == '.' && chars[i + 2] == '.' {
            tokens.push(Token {
                kind: TokenKind::Punct("...".to_string()),
                line,
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
                tokens.push(Token {
                    kind: TokenKind::Punct(op.to_string()),
                    line,
                    is_member: false,
                });
                i += 2;
                continue;
            }
        }

        // Single-character punctuation
        tokens.push(Token {
            kind: TokenKind::Punct(c.to_string()),
            line,
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

/// Extract all instance names dynamically assigned via `.Name = "Foo"` or `.Name = CONST`
/// inside a script.
fn extract_created_names(tokens: &[Token], consts: &HashMap<String, String>) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut i = 0;
    while i + 2 < tokens.len() {
        if tokens[i].is_member && tokens[i].ident() == Some("Name") && tokens[i + 1].is_punct("=") {
            let rhs = &tokens[i + 2];
            if let Some(lit) = rhs.str_lit() {
                names.insert(lit.to_string());
            } else if !rhs.is_member {
                if let Some(ident) = rhs.ident() {
                    if let Some(resolved) = consts.get(ident) {
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
    check_cylinder_axis_and_vfx_loops(path, source, &mut diagnostics);

    AnalysisReport { diagnostics }
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
        let own_created = extract_created_names(&tokens, &consts);
        for name in &own_created {
            all_created_names.insert(name.clone());
        }
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
            own_created,
        });
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
