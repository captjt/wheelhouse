//! Tokens spent, by when each model call happened, which model and which
//! provider: read from the agents' own transcripts (Claude Code's, Codex's
//! rollouts, Antigravity's), so every kind of work counts, not just
//! reviews. Each file is read once and then only for what it appended.
//! Nothing here is stored or goes on the record.

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use crate::review::agentcli;

/// One model call's tokens: `[input, cache_read, cache_write, output]`,
/// input not counting what came from the cache.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    /// Unix seconds.
    pub at: i64,
    /// The model's name for people: `Opus 5.5`, `GPT-6.2 Astra`.
    pub model: String,
    /// `anthropic`, `openai`, `google`, or what Codex names.
    pub provider: String,
    pub tokens: [i64; 4],
}

impl Call {
    pub fn total(&self) -> i64 {
        self.tokens.iter().sum()
    }
}

/// What's been read of one file, and where reading stopped.
#[derive(Default, Clone)]
struct Progress {
    offset: u64,
    mtime: Option<SystemTime>,
    calls: Vec<Call>,
    /// Claude Code: responses already counted (a response in several
    /// blocks repeats its usage on each line).
    seen: HashSet<String>,
    /// Codex: the model and provider in effect, and the running total.
    model: Option<String>,
    provider: Option<String>,
    total: Option<[i64; 4]>,
}

fn cache() -> &'static Mutex<HashMap<PathBuf, Progress>> {
    static C: OnceLock<Mutex<HashMap<PathBuf, Progress>>> = OnceLock::new();
    C.get_or_init(Mutex::default)
}

/// The calls in a transcript of `app` (`claude`, `codex`, `agy`), read
/// again only from where it was last read, and only when it changed. A
/// file last written before `from` (unix seconds) has no calls from then
/// on, and isn't read.
pub fn calls(app: &str, path: &Path, from: i64) -> Vec<Call> {
    let meta = std::fs::metadata(path).ok();
    let written = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
    if written.is_some_and(|w| w < from) {
        return Vec::new();
    }
    let Some(meta) = meta else {
        // An archived Codex rollout: read whole, once.
        let z = PathBuf::from(format!("{}.zst", path.display()));
        if app == "codex" && z.is_file() {
            return archived(&z);
        }
        return Vec::new();
    };
    let (len, mtime) = (meta.len(), meta.modified().ok());
    let mut r = cache().lock().unwrap_or_else(|e| e.into_inner()).remove(path).unwrap_or_default();
    // Rewritten rather than appended to: start over.
    if len < r.offset {
        r = Progress::default();
    }
    if r.offset < len || r.mtime != mtime {
        if let Ok(mut f) = std::fs::File::open(path)
            && f.seek(SeekFrom::Start(r.offset)).is_ok()
        {
            let left = len - r.offset;
            let read = read_lines(app, &mut r, std::io::BufReader::new(f.take(left)));
            r.offset += read;
        }
        r.mtime = mtime;
    }
    let out = r.calls.clone();
    cache().lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_path_buf(), r);
    out
}

fn archived(z: &Path) -> Vec<Call> {
    let key = z.to_path_buf();
    if let Some(r) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return r.calls.clone();
    }
    let mut r = Progress::default();
    let child = std::process::Command::new("zstd")
        .arg("-dcq")
        .arg(z)
        .env("PATH", crate::review::github::command_path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Ok(mut c) = child {
        if let Some(out) = c.stdout.take() {
            read_lines("codex", &mut r, std::io::BufReader::new(out));
        }
        let _ = c.wait();
    }
    let calls = r.calls.clone();
    cache().lock().unwrap_or_else(|e| e.into_inner()).insert(key, r);
    calls
}

fn epoch(at: &str) -> Option<i64> {
    crate::time::parse(at).map(|t| t.unix_timestamp())
}

/// Reads whole lines, one at a time, and says how many bytes they were: a
/// line still being written (no newline yet) is left for next time. A line
/// that isn't UTF-8 is read as well as it can be, never the end of the file.
fn read_lines(app: &str, r: &mut Progress, mut from: impl BufRead) -> u64 {
    let (mut done, mut buf) = (0u64, Vec::new());
    loop {
        buf.clear();
        match from.read_until(b'\n', &mut buf) {
            Ok(n) if n > 0 && buf.ends_with(b"\n") => done += n as u64,
            _ => return done,
        }
        let line = String::from_utf8_lossy(&buf);
        match app {
            "claude" => claude_line(r, &line),
            "codex" => codex_line(r, &line),
            "agy" => agy_line(r, &line),
            _ => {}
        }
    }
}

fn claude_line(r: &mut Progress, line: &str) {
    if !line.contains("\"usage\"") {
        return;
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else { return };
    let m = &v["message"];
    let u = &m["usage"];
    if v["type"] != "assistant" || u.is_null() {
        return;
    }
    // `<synthetic>` is Claude Code's own message, not a model's.
    let Some(model) = m["model"].as_str().filter(|x| x.starts_with("claude-")) else { return };
    if let Some(id) = m["id"].as_str()
        && !r.seen.insert(id.to_string())
    {
        return;
    }
    let Some(at) = v["timestamp"].as_str().and_then(epoch) else { return };
    let n = |k: &str| u[k].as_i64().unwrap_or(0);
    let tokens = [n("input_tokens"), n("cache_read_input_tokens"), n("cache_creation_input_tokens"), n("output_tokens")];
    push(r, Call { at, model: agentcli::claude_model_name(model).unwrap_or_else(|| model.to_string()), provider: "anthropic".into(), tokens });
}

fn codex_line(r: &mut Progress, line: &str) {
    let what = if line.contains("\"token_count\"") {
        "count"
    } else if line.contains("\"turn_context\"") || line.contains("\"session_meta\"") {
        "context"
    } else {
        return;
    };
    let Ok(v) = serde_json::from_str::<Value>(line) else { return };
    let p = &v["payload"];
    if what == "context" {
        if let Some(m) = p["model"].as_str().filter(|m| !m.is_empty()) {
            r.model = Some(m.to_string());
        }
        if let Some(m) = p["model_provider"].as_str().filter(|m| !m.is_empty()) {
            r.provider = Some(m.to_string());
        }
        return;
    }
    let info = &p["info"];
    if p["type"] != "token_count" || info.is_null() {
        return;
    }
    let four = |t: &Value| {
        let n = |k: &str| t[k].as_i64().unwrap_or(0);
        let cached = n("cached_input_tokens");
        [(n("input_tokens") - cached).max(0), cached, n("cache_write_input_tokens"), n("output_tokens")]
    };
    let total = four(&info["total_token_usage"]);
    // The running total's step is this call's. The file's first count (a
    // rollout can start mid-session, its total carried over), or one where
    // the total went back (a new context), is the call's own count.
    let own = if info["last_token_usage"].is_object() { four(&info["last_token_usage"]) } else { total };
    let tokens = match r.total {
        Some(prev) if (0..4).all(|i| total[i] >= prev[i]) => [total[0] - prev[0], total[1] - prev[1], total[2] - prev[2], total[3] - prev[3]],
        _ => own,
    };
    r.total = Some(total);
    // The same count again (a rate-limit update) isn't a call.
    if tokens.iter().all(|t| *t == 0) {
        return;
    }
    let Some(at) = v["timestamp"].as_str().and_then(epoch) else { return };
    let model = r.model.clone().unwrap_or_else(|| "Codex".into());
    push(r, Call { at, model: codex_model_name(&model), provider: r.provider.clone().unwrap_or_else(|| "openai".into()), tokens });
}

fn agy_line(r: &mut Progress, line: &str) {
    if !line.contains("PLANNER_RESPONSE") {
        return;
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else { return };
    if v["type"] != "PLANNER_RESPONSE" {
        return;
    }
    let n = |k: &str| v[k].as_i64().or_else(|| v[k].as_str().and_then(|s| s.parse().ok())).unwrap_or(0);
    let tokens = [n("input_tokens"), n("cache_read_tokens"), n("cache_write_tokens"), n("output_tokens")];
    if tokens.iter().all(|t| *t == 0) {
        return;
    }
    if let Some(m) = v["model"].as_str().filter(|m| !m.is_empty()) {
        r.model = Some(agentcli::agy_model_name(m).0);
    }
    let Some(at) = v["created_at"].as_str().and_then(epoch) else { return };
    push(r, Call { at, model: r.model.clone().unwrap_or_else(|| "Gemini".into()), provider: "google".into(), tokens });
}

/// Calls by the same model in the same minute are kept as one: a long
/// session is thousands of calls.
fn push(r: &mut Progress, c: Call) {
    if let Some(last) = r.calls.last_mut()
        && last.at / 60 == c.at / 60
        && last.model == c.model
        && last.provider == c.provider
    {
        for i in 0..4 {
            last.tokens[i] += c.tokens[i];
        }
        return;
    }
    r.calls.push(c);
}

/// `gpt-6.2-astra` → `GPT-6.2 Astra`; `o4-mini` → `o4 Mini`.
pub fn codex_model_name(id: &str) -> String {
    let (head, rest) = match id.strip_prefix("gpt-") {
        Some(r) => {
            let mut parts = r.splitn(2, '-');
            (format!("GPT-{}", parts.next().unwrap_or("")), parts.next().unwrap_or("").to_string())
        }
        None => {
            let mut parts = id.splitn(2, '-');
            (parts.next().unwrap_or(id).to_string(), parts.next().unwrap_or("").to_string())
        }
    };
    let words: Vec<String> = rest
        .split('-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        })
        .collect();
    if words.is_empty() { head } else { format!("{head} {}", words.join(" ")) }
}

/// A provider's name for people.
pub fn provider_name(p: &str) -> String {
    match p {
        "anthropic" => "Anthropic".into(),
        "openai" => "OpenAI".into(),
        "google" => "Google".into(),
        "azure" => "Azure OpenAI".into(),
        "ollama" => "Ollama".into(),
        "openrouter" => "OpenRouter".into(),
        other => {
            let mut c = other.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wh-usage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(dir: &Path, name: &str, lines: &[Value]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        p
    }

    fn append(p: &Path, lines: &[Value]) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    #[test]
    fn claude_counts_each_response_once_and_reads_only_what_was_appended() {
        let dir = scratch("claude");
        let usage = json!({ "input_tokens": 10, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 5, "output_tokens": 20 });
        let p = write(
            &dir,
            "s.jsonl",
            &[
                json!({ "type": "user", "timestamp": "2026-11-20T17:00:00Z" }),
                json!({ "type": "assistant", "timestamp": "2026-11-20T17:00:05Z", "message": { "id": "m1", "model": "claude-opus-5-5", "content": [], "usage": usage } }),
                json!({ "type": "assistant", "timestamp": "2026-11-20T17:00:06Z", "message": { "id": "m1", "model": "claude-opus-5-5", "content": [], "usage": usage } }),
                json!({ "type": "assistant", "timestamp": "2026-11-20T17:00:07Z", "message": { "id": "m0", "model": "<synthetic>", "usage": usage } }),
            ],
        );
        let c = calls("claude", &p, 0);
        assert_eq!(c, vec![Call { at: 1_795_194_005, model: "Opus 5.5".into(), provider: "anthropic".into(), tokens: [10, 100, 5, 20] }]);

        // A later response, a minute on, and a line still being written.
        append(&p, &[json!({ "type": "assistant", "timestamp": "2026-11-20T17:01:30Z", "message": { "id": "m2", "model": "claude-sonnet-5-5", "usage": usage } })]);
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
            write!(f, "{{\"type\":\"assistant\",\"message\":{{\"usage\"").unwrap();
        }
        let c = calls("claude", &p, 0);
        assert_eq!(c.len(), 2);
        assert_eq!((c[1].model.as_str(), c[1].total()), ("Sonnet 5.5", 135));
    }

    #[test]
    fn codex_counts_the_running_totals_steps_with_its_model_and_provider() {
        let dir = scratch("codex");
        let count = |at: &str, total: [i64; 3], last: [i64; 3]| {
            let u = |t: [i64; 3]| json!({ "input_tokens": t[0], "cached_input_tokens": t[1], "cache_write_input_tokens": 0, "output_tokens": t[2] });
            json!({ "timestamp": at, "type": "event_msg", "payload": { "type": "token_count", "info": { "total_token_usage": u(total), "last_token_usage": u(last) } } })
        };
        let p = write(
            &dir,
            "rollout.jsonl",
            &[
                json!({ "timestamp": "2026-11-24T02:07:57Z", "type": "session_meta", "payload": { "model_provider": "openai" } }),
                json!({ "timestamp": "2026-11-24T02:07:58Z", "type": "turn_context", "payload": { "model": "gpt-6.2-astra" } }),
                // Its first count: the call's own, not the total it carried in.
                count("2026-11-24T02:08:34Z", [5300, 100, 7], [300, 100, 7]),
                count("2026-11-24T02:08:35Z", [5300, 100, 7], [300, 100, 7]),
                count("2026-11-24T02:09:41Z", [5620, 400, 20], [320, 300, 13]),
                // Compacted: the total starts again.
                count("2026-11-24T02:10:41Z", [50, 0, 2], [50, 0, 2]),
            ],
        );
        let c = calls("codex", &p, 0);
        assert_eq!(c.iter().map(|c| c.tokens).collect::<Vec<_>>(), vec![[200, 100, 0, 7], [20, 300, 0, 13], [50, 0, 0, 2]]);
        assert!(c.iter().all(|c| c.model == "GPT-6.2 Astra" && c.provider == "openai"));
    }

    #[test]
    fn antigravity_counts_each_planner_response() {
        let dir = scratch("agy");
        let p = write(
            &dir,
            "transcript.jsonl",
            &[
                json!({ "type": "USER_INPUT", "created_at": "2026-11-23T23:57:30Z" }),
                json!({ "type": "PLANNER_RESPONSE", "created_at": "2026-11-23T23:57:36Z", "model": "gemini-4-neon-high", "input_tokens": 120, "cache_read_tokens": "300", "output_tokens": 15 }),
            ],
        );
        let c = calls("agy", &p, 0);
        assert_eq!((c[0].model.as_str(), c[0].provider.as_str(), c[0].tokens), ("Gemini 4 Neon", "google", [120, 300, 0, 15]));
    }

    #[test]
    fn a_file_last_written_before_the_period_isnt_read() {
        let dir = scratch("old");
        let usage = json!({ "input_tokens": 1, "output_tokens": 1 });
        let p =
            write(&dir, "s.jsonl", &[json!({ "type": "assistant", "timestamp": "2026-11-20T17:00:05Z", "message": { "id": "m1", "model": "claude-opus-5-5", "usage": usage } })]);
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs() as i64;
        assert!(calls("claude", &p, now + 3600).is_empty());
        assert_eq!(calls("claude", &p, now - 3600).len(), 1);
    }

    #[test]
    fn a_missing_rewritten_or_garbled_transcript_doesnt_stop_the_count() {
        let dir = scratch("bad");
        assert!(calls("claude", &dir.join("gone.jsonl"), 0).is_empty());
        assert!(calls("codex", &dir.join("gone.jsonl"), 0).is_empty(), "no archived rollout either");
        let line = |id: &str, at: &str| json!({ "type": "assistant", "timestamp": at, "message": { "id": id, "model": "claude-opus-5-5", "usage": { "input_tokens": 1, "output_tokens": 1 } } });
        let p = dir.join("s.jsonl");
        // Not JSON, then not UTF-8, then a call: the call still counts.
        let mut bytes = b"{not json\n\xff\xfe\"usage\"\n".to_vec();
        bytes.extend(format!("{}\n", line("m1", "2026-11-20T17:00:05Z")).bytes());
        std::fs::write(&p, &bytes).unwrap();
        assert_eq!(calls("claude", &p, 0).len(), 1);
        // Rewritten shorter (a new session in its place): read again from the start.
        std::fs::write(&p, format!("{}\n", line("m9", "2026-11-21T09:00:00Z"))).unwrap();
        let c = calls("claude", &p, 0);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].at, secs("2026-11-21T09:00:00Z"));
    }

    fn secs(at: &str) -> i64 {
        crate::time::parse(at).unwrap().unix_timestamp()
    }

    #[test]
    fn model_and_provider_names() {
        assert_eq!(codex_model_name("gpt-6.2-astra"), "GPT-6.2 Astra");
        assert_eq!(codex_model_name("gpt-5"), "GPT-5");
        assert_eq!(codex_model_name("o4-mini"), "o4 Mini");
        assert_eq!(provider_name("openai"), "OpenAI");
        assert_eq!(provider_name("mistral"), "Mistral");
    }
}
