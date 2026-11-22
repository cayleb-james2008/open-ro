//! Append-only Teamwork message board persisted as JSONL (`board.jsonl`).
//!
//! Models an Antigravity-style swarm board: strictly monotonic `seq` numbers,
//! `@name` / `@all` mentions for targeted wakeups, `[done]` detection, and
//! crash-safe JSONL append + reload.

use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardKind {
    Goal,
    Brief,
    Post,
    Done,
    Verdict,
    Claim,
    Human,
    Quota,
    System,
}

impl BoardKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BoardKind::Goal => "goal",
            BoardKind::Brief => "brief",
            BoardKind::Post => "post",
            BoardKind::Done => "done",
            BoardKind::Verdict => "verdict",
            BoardKind::Claim => "claim",
            BoardKind::Human => "human",
            BoardKind::Quota => "quota",
            BoardKind::System => "system",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "goal" => BoardKind::Goal,
            "brief" => BoardKind::Brief,
            "done" => BoardKind::Done,
            "verdict" => BoardKind::Verdict,
            "claim" => BoardKind::Claim,
            "human" => BoardKind::Human,
            "quota" => BoardKind::Quota,
            "system" => BoardKind::System,
            _ => BoardKind::Post,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardMessage {
    pub seq: u64,
    pub ts_ms: u64,
    pub time: String,
    pub author: String,
    pub kind: BoardKind,
    pub text: String,
    pub mentions: Vec<String>,
    pub is_done: bool,
    pub task_id: Option<String>,
}

impl BoardMessage {
    pub fn to_json(&self) -> Value {
        json!({
            "seq": self.seq,
            "ts_ms": self.ts_ms,
            "time": self.time,
            "author": self.author,
            "kind": self.kind.as_str(),
            "text": self.text,
            "mentions": self.mentions,
            "is_done": self.is_done,
            "task_id": self.task_id,
        })
    }

    pub fn from_json(v: &Value) -> Option<Self> {
        let seq = v.get("seq")?.as_u64()?;
        let ts_ms = v.get("ts_ms").and_then(|x| x.as_u64()).unwrap_or(0);
        let time = v
            .get("time")
            .and_then(|x| x.as_str())
            .map(String::from)
            .unwrap_or_else(|| format_iso_ms(ts_ms));
        let author = v.get("author")?.as_str()?.to_string();
        let kind = v
            .get("kind")
            .and_then(|x| x.as_str())
            .map(BoardKind::parse)
            .unwrap_or(BoardKind::Post);
        let text = v.get("text")?.as_str()?.to_string();
        let mentions = v
            .get("mentions")
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.as_str().map(|s| s.to_ascii_lowercase()))
                    .collect()
            })
            .unwrap_or_else(|| parse_mentions(&text));
        let is_done = v
            .get("is_done")
            .and_then(|x| x.as_bool())
            .unwrap_or_else(|| is_done_post(&text));
        let task_id = v
            .get("task_id")
            .and_then(|x| x.as_str())
            .map(String::from);
        Some(BoardMessage {
            seq,
            ts_ms,
            time,
            author,
            kind,
            text,
            mentions,
            is_done,
            task_id,
        })
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn format_iso_ms(ts_ms: u64) -> String {
    let secs = (ts_ms / 1000) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Extract `@name` / `@all` mentions (lowercased, deduplicated in order).
/// Ignores email addresses where `@` follows an identifier or dot character.
pub fn parse_mentions(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let boundary_ok = i == 0 || {
                let prev = bytes[i - 1];
                !prev.is_ascii_alphanumeric() && prev != b'_' && prev != b'-' && prev != b'.'
            };
            if boundary_ok && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphanumeric() {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'_' | b'-'))
                    && end - start < 32
                {
                    end += 1;
                }
                let name = text[start..end].to_ascii_lowercase();
                if !out.contains(&name) {
                    out.push(name);
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// True when a post starts with `[done]` after optional leading `@mention` tokens.
pub fn is_done_post(text: &str) -> bool {
    let mut s = text.trim_start();
    loop {
        if let Some(rest) = s.strip_prefix('@') {
            let mut end = 0;
            for (idx, ch) in rest.char_indices() {
                if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                    end = idx + ch.len_utf8();
                } else {
                    break;
                }
            }
            if end == 0 {
                break;
            }
            s = rest[end..].trim_start_matches(|c: char| c.is_whitespace() || c == ',' || c == ':');
            continue;
        }
        break;
    }
    let low = s.to_ascii_lowercase();
    if let Some(after) = low.strip_prefix("[done]") {
        after.is_empty()
            || after.starts_with(char::is_whitespace)
            || after.starts_with(':')
            || after.starts_with('-')
    } else {
        false
    }
}

pub struct Board {
    path: Option<PathBuf>,
    messages: Vec<BoardMessage>,
    next_seq: u64,
}

impl Board {
    pub fn in_memory() -> Self {
        Board {
            path: None,
            messages: Vec::new(),
            next_seq: 1,
        }
    }

    /// Open (or create) an append-only `board.jsonl` file.
    /// Ignores a truncated final line left by an abrupt crash while preserving all prior entries.
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let mut messages = Vec::new();
        let mut max_seq = 0u64;
        if path.exists() {
            let file = File::open(&path).map_err(|e| format!("open board {}: {e}", path.display()))?;
            let lines: Vec<String> = BufReader::new(file)
                .lines()
                .map_while(Result::ok)
                .collect();
            let total = lines.len();
            for (idx, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match serde_json::from_str::<Value>(trimmed)
                    .ok()
                    .and_then(|v| BoardMessage::from_json(&v))
                {
                    Some(msg) => {
                        if msg.seq > max_seq {
                            max_seq = msg.seq;
                        }
                        messages.push(msg);
                    }
                    None if idx + 1 == total => {
                        eprintln!(
                            "openro: ignored truncated tail line in {}",
                            path.display()
                        );
                    }
                    None => {
                        return Err(format!(
                            "corrupt board line {} in {}",
                            idx + 1,
                            path.display()
                        ));
                    }
                }
            }
        }
        Ok(Board {
            path: Some(path),
            messages,
            next_seq: max_seq + 1,
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn latest_seq(&self) -> u64 {
        self.messages.last().map(|m| m.seq).unwrap_or(0)
    }

    pub fn all(&self) -> &[BoardMessage] {
        &self.messages
    }

    pub fn since(&self, since_seq: u64) -> Vec<BoardMessage> {
        self.messages
            .iter()
            .filter(|m| m.seq > since_seq)
            .cloned()
            .collect()
    }

    /// True if any message posted after `since_seq` by another author mentions `agent_name` or `@all`.
    pub fn mentions_for_since(&self, agent_name: &str, since_seq: u64) -> bool {
        let target = agent_name.to_ascii_lowercase();
        self.messages.iter().any(|m| {
            m.seq > since_seq
                && !m.author.eq_ignore_ascii_case(&target)
                && m.mentions.iter().any(|x| x == &target || x == "all")
        })
    }

    pub fn append(
        &mut self,
        author: &str,
        kind: BoardKind,
        text: &str,
        task_id: Option<String>,
    ) -> Result<BoardMessage, String> {
        let ts_ms = now_ms();
        let mentions = parse_mentions(text);
        let is_done = is_done_post(text);
        let effective_kind = if is_done && kind == BoardKind::Post {
            BoardKind::Done
        } else {
            kind
        };
        let msg = BoardMessage {
            seq: self.next_seq,
            ts_ms,
            time: format_iso_ms(ts_ms),
            author: author.trim().to_ascii_lowercase(),
            kind: effective_kind,
            text: text.to_string(),
            mentions,
            is_done,
            task_id,
        };
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("create board dir {}: {e}", parent.display()))?;
            }
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| format!("open board {}: {e}", path.display()))?;
            let line = format!("{}\n", msg.to_json());
            file.write_all(line.as_bytes())
                .and_then(|_| file.flush())
                .map_err(|e| format!("append board {}: {e}", path.display()))?;
        }
        self.next_seq += 1;
        self.messages.push(msg.clone());
        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_parsing_ignores_emails_and_dedups() {
        let mentions = parse_mentions("Hey @Systems, ask @verifier and @systems; ignore foo@bar.com (@UI) @all");
        assert_eq!(mentions, vec!["systems", "verifier", "ui", "all"]);
        assert!(parse_mentions("no mentions here @").is_empty());
    }

    #[test]
    fn done_post_detection() {
        assert!(is_done_post("[done] finished Net"));
        assert!(is_done_post("@coordinator [done] built systems"));
        assert!(is_done_post("@verifier, @coordinator: [DONE] - ready"));
        assert!(!is_done_post("@systems working on [done] later"));
        assert!(!is_done_post("in progress"));
    }

    #[test]
    fn jsonl_append_reload_continues_seq() {
        let dir = std::env::temp_dir().join(format!("openro-board-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("board.jsonl");
        {
            let mut b = Board::open(path.clone()).unwrap();
            let m1 = b.append("human", BoardKind::Goal, "@coordinator build game", None).unwrap();
            let m2 = b.append("coordinator", BoardKind::Post, "@systems take t1", Some("t1".into())).unwrap();
            assert_eq!(m1.seq, 1);
            assert_eq!(m2.seq, 2);
        }
        // Simulate trailing partial line from a crash
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(f, "{{\"seq\":3,\"broken").unwrap();

        let mut reloaded = Board::open(path).unwrap();
        assert_eq!(reloaded.latest_seq(), 2);
        assert!(reloaded.mentions_for_since("systems", 1));
        assert!(!reloaded.mentions_for_since("ui", 0));
        let m3 = reloaded.append("systems", BoardKind::Post, "@verifier [done] t1 done", Some("t1".into())).unwrap();
        assert_eq!(m3.seq, 3);
        assert!(m3.is_done);
        assert_eq!(m3.kind, BoardKind::Done);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
