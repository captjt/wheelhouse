//! SQLite. `entries` is the truth. `dockets` holds each docket's folded
//! state for speed and can be rebuilt from `entries` at any time.

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

use crate::record::{DocketState, Stored, chain, hex};

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Message {
    pub id: i64,
    pub docket: Option<String>,
    pub seq: Option<i64>,
    pub from_addr: String,
    pub from_block: Option<String>,
    pub to_addr: String,
    pub to_block: Option<String>,
    pub subject: Option<String>,
    pub body: String,
    pub options: Vec<String>,
    pub reply_to: Option<i64>,
    pub choice: Option<String>,
    pub status: String,
    pub why: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// A preview the message carries: its url, local url, ports, and why
    /// it came down.
    pub preview: Option<serde_json::Value>,
}

pub struct Store {
    conn: Connection,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS entries (
  docket TEXT NOT NULL,
  seq INTEGER NOT NULL,
  id TEXT NOT NULL,          -- sha256:<hex> of envelope
  chain TEXT NOT NULL,       -- hex
  kind TEXT NOT NULL,
  at TEXT NOT NULL,
  keyid TEXT NOT NULL,
  envelope TEXT NOT NULL,    -- the exact bytes
  PRIMARY KEY (docket, seq)
);
CREATE TABLE IF NOT EXISTS dockets (  -- derived
  id TEXT PRIMARY KEY,
  n INTEGER NOT NULL,
  state TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS messages (  -- numbering and delivery; the content is also on the record
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  docket TEXT,
  seq INTEGER,
  from_addr TEXT NOT NULL,
  from_block TEXT,
  to_addr TEXT NOT NULL,
  to_block TEXT,
  subject TEXT,
  body TEXT NOT NULL,
  options TEXT NOT NULL DEFAULT '[]',
  reply_to INTEGER,
  choice TEXT,
  status TEXT NOT NULL,   -- open (to you) | queued | delivered | read | answered | done | failed
  why TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS push_subs (
  endpoint TEXT PRIMARY KEY,
  p256dh TEXT NOT NULL,
  auth TEXT NOT NULL,
  agent TEXT,
  created_at TEXT NOT NULL,
  last_ok TEXT,
  failures INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS review_prs (  -- the Review protocol's working state per PR; the story is on its docket
  key TEXT PRIMARY KEY,      -- owner/name#n
  repo TEXT NOT NULL,
  pr INTEGER NOT NULL,
  docket TEXT,
  body TEXT NOT NULL,        -- JSON
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS keys (
  keyid TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  added_at TEXT NOT NULL
);
";

/// Columns added after a table was first made.
fn migrate(conn: &Connection) -> Result<()> {
    let has = |table: &str, col: &str| -> Result<bool> {
        let mut st = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let cols: Vec<String> = st.query_map([], |r| r.get::<_, String>(1))?.collect::<rusqlite::Result<_>>()?;
        Ok(cols.iter().any(|c| c == col))
    };
    if !has("messages", "preview")? {
        conn.execute_batch("ALTER TABLE messages ADD COLUMN preview TEXT")?;
    }
    Ok(())
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Store { conn })
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Store { conn })
    }

    pub fn meta_get(&self, k: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT v FROM meta WHERE k = ?1", params![k], |r| r.get::<_, String>(0)).optional()?)
    }

    pub fn meta_set(&mut self, k: &str, v: &str) -> Result<()> {
        self.conn.execute("INSERT INTO meta (k, v) VALUES (?1, ?2) ON CONFLICT (k) DO UPDATE SET v = ?2", params![k, v])?;
        Ok(())
    }

    /// Every PR the Review protocol knows: (key, docket, body), by repo then number.
    pub fn review_prs(&self) -> Result<Vec<(String, Option<String>, String)>> {
        let mut st = self.conn.prepare("SELECT key, docket, body FROM review_prs ORDER BY repo, pr")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn review_pr(&self, key: &str) -> Result<Option<(Option<String>, String)>> {
        Ok(self.conn.query_row("SELECT docket, body FROM review_prs WHERE key = ?1", params![key], |r| Ok((r.get(0)?, r.get(1)?))).optional()?)
    }

    pub fn put_review_pr(&mut self, key: &str, repo: &str, pr: i64, docket: Option<&str>, body: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO review_prs (key, repo, pr, docket, body, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (key) DO UPDATE SET docket = COALESCE(?4, docket), body = ?5, updated_at = ?6",
            params![key, repo, pr, docket, body, crate::time::now()],
        )?;
        Ok(())
    }

    /// The next docket number, as `D-n`.
    pub fn next_docket(&mut self) -> Result<(String, i64)> {
        let tx = self.conn.transaction()?;
        let n: i64 = tx.query_row("SELECT v FROM meta WHERE k = 'docket_n'", [], |r| r.get::<_, String>(0)).optional()?.map_or(0, |v| {
            v.parse().unwrap_or(0)
        }) + 1;
        tx.execute("INSERT INTO meta (k, v) VALUES ('docket_n', ?1) ON CONFLICT (k) DO UPDATE SET v = ?1", params![n.to_string()])?;
        tx.commit()?;
        Ok((format!("D-{n}"), n))
    }

    /// Append an envelope to a docket: give it the next seq and chain it.
    pub fn append(&mut self, docket: &str, envelope: &str, kind: &str, at: &str, keyid: &str) -> Result<Stored> {
        let tx = self.conn.transaction()?;
        let last: Option<(i64, String)> = tx
            .query_row("SELECT seq, chain FROM entries WHERE docket = ?1 ORDER BY seq DESC LIMIT 1", params![docket], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        let (seq, prev) = match last {
            None => (1, chain::GENESIS),
            Some((s, c)) => (s + 1, chain::parse_chain(&c).context("a stored chain value isn't hex")?),
        };
        let id = chain::entry_id(envelope.as_bytes());
        let next = hex::encode(&chain::next(&prev, &id));
        let id = chain::id_string(&id);
        tx.execute(
            "INSERT INTO entries (docket, seq, id, chain, kind, at, keyid, envelope) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![docket, seq, id, next, kind, at, keyid, envelope],
        )?;
        tx.commit()?;
        Ok(Stored { docket: docket.to_string(), seq, id, chain: next, envelope: envelope.to_string() })
    }

    pub fn entries(&self, docket: &str) -> Result<Vec<Stored>> {
        let mut st = self.conn.prepare("SELECT docket, seq, id, chain, envelope FROM entries WHERE docket = ?1 ORDER BY seq")?;
        let rows = st.query_map(params![docket], |r| {
            Ok(Stored { docket: r.get(0)?, seq: r.get(1)?, id: r.get(2)?, chain: r.get(3)?, envelope: r.get(4)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn save_state(&mut self, st: &DocketState) -> Result<()> {
        let n: i64 = st.id.strip_prefix("D-").and_then(|n| n.parse().ok()).unwrap_or(0);
        self.conn.execute(
            "INSERT INTO dockets (id, n, state) VALUES (?1, ?2, ?3) ON CONFLICT (id) DO UPDATE SET state = ?3",
            params![st.id, n, serde_json::to_string(st)?],
        )?;
        Ok(())
    }

    pub fn state(&self, id: &str) -> Result<Option<DocketState>> {
        let s: Option<String> = self.conn.query_row("SELECT state FROM dockets WHERE id = ?1", params![id], |r| r.get(0)).optional()?;
        Ok(match s {
            Some(s) => Some(serde_json::from_str(&s)?),
            None => None,
        })
    }

    /// Every docket, newest first.
    pub fn states(&self) -> Result<Vec<DocketState>> {
        let mut st = self.conn.prepare("SELECT state FROM dockets ORDER BY n DESC")?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for s in rows {
            out.push(serde_json::from_str(&s?)?);
        }
        Ok(out)
    }

    pub fn docket_ids(&self) -> Result<Vec<String>> {
        let mut st = self.conn.prepare("SELECT DISTINCT docket FROM entries")?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn insert_message(&mut self, m: &Message) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO messages (docket, seq, from_addr, from_block, to_addr, to_block, subject, body, options, reply_to, choice, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
            params![m.docket, m.seq, m.from_addr, m.from_block, m.to_addr, m.to_block, m.subject, m.body, serde_json::to_string(&m.options)?, m.reply_to, m.choice, m.status, m.created_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn message(&self, id: i64) -> Result<Option<Message>> {
        Ok(self.messages_where("id = ?1", params![id])?.into_iter().next())
    }

    pub fn messages_where(&self, cond: &str, args: impl rusqlite::Params) -> Result<Vec<Message>> {
        let sql = format!(
            "SELECT id, docket, seq, from_addr, from_block, to_addr, to_block, subject, body, options, reply_to, choice, status, why, created_at, updated_at, preview
             FROM messages WHERE {cond} ORDER BY id"
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(args, |r| {
            Ok(Message {
                id: r.get(0)?,
                docket: r.get(1)?,
                seq: r.get(2)?,
                from_addr: r.get(3)?,
                from_block: r.get(4)?,
                to_addr: r.get(5)?,
                to_block: r.get(6)?,
                subject: r.get(7)?,
                body: r.get(8)?,
                options: serde_json::from_str(&r.get::<_, String>(9)?).unwrap_or_default(),
                reply_to: r.get(10)?,
                choice: r.get(11)?,
                status: r.get(12)?,
                why: r.get(13)?,
                created_at: r.get(14)?,
                updated_at: r.get(15)?,
                preview: r.get::<_, Option<String>>(16)?.and_then(|p| serde_json::from_str(&p).ok()),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_message_preview(&mut self, id: i64, preview: &serde_json::Value) -> Result<()> {
        self.conn.execute("UPDATE messages SET preview = ?2 WHERE id = ?1", params![id, preview.to_string()])?;
        Ok(())
    }

    pub fn set_message_preview_down(&mut self, id: i64, why: &str) -> Result<()> {
        let Some(m) = self.message(id)? else { return Ok(()) };
        let mut p = m.preview.unwrap_or_default();
        p["down"] = serde_json::json!(why);
        self.set_message_preview(id, &p)
    }

    pub fn set_message_place(&mut self, id: i64, docket: &str, seq: i64) -> Result<()> {
        self.conn.execute("UPDATE messages SET docket = ?2, seq = ?3 WHERE id = ?1", params![id, docket, seq])?;
        Ok(())
    }

    pub fn set_message_status(&mut self, id: i64, status: &str, why: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE messages SET status = ?2, why = ?3, updated_at = ?4 WHERE id = ?1", params![id, status, why, crate::time::now_ms()])?;
        Ok(())
    }

    pub fn push_subscribe(&mut self, endpoint: &str, p256dh: &str, auth: &str, agent: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO push_subs (endpoint, p256dh, auth, agent, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(endpoint) DO UPDATE SET p256dh = ?2, auth = ?3, agent = ?4, failures = 0",
            params![endpoint, p256dh, auth, agent, crate::time::now()],
        )?;
        Ok(())
    }

    pub fn push_unsubscribe(&mut self, endpoint: &str) -> Result<()> {
        self.conn.execute("DELETE FROM push_subs WHERE endpoint = ?1", [endpoint])?;
        Ok(())
    }

    /// `(endpoint, p256dh, auth)` for every subscribed device.
    pub fn push_subs(&self) -> Result<Vec<(String, String, String)>> {
        let mut st = self.conn.prepare("SELECT endpoint, p256dh, auth FROM push_subs")?;
        Ok(st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn push_result(&mut self, endpoint: &str, ok: bool) -> Result<()> {
        let sql = if ok {
            "UPDATE push_subs SET failures = 0, last_ok = ?2 WHERE endpoint = ?1"
        } else {
            "UPDATE push_subs SET failures = failures + 1 WHERE endpoint = ?1 AND ?2 IS NOT NULL"
        };
        self.conn.execute(sql, params![endpoint, crate::time::now()])?;
        Ok(())
    }

    pub fn put_key(&mut self, keyid: &str, name: &str, at: &str) -> Result<()> {
        let have: Option<String> =
            self.conn.query_row("SELECT name FROM keys WHERE keyid = ?1", params![keyid], |r| r.get(0)).optional()?;
        match have {
            Some(n) if n != name => bail!("key {keyid} is already {n}"),
            Some(_) => Ok(()),
            None => {
                self.conn.execute("INSERT INTO keys (keyid, name, added_at) VALUES (?1, ?2, ?3)", params![keyid, name, at])?;
                Ok(())
            }
        }
    }

    /// Every key the record knows: `(keyid, name)`.
    pub fn keys(&self) -> Result<Vec<(String, String)>> {
        let mut st = self.conn.prepare("SELECT keyid, name FROM keys ORDER BY added_at")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
