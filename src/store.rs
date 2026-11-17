//! SQLite. `entries` is the truth. `dockets` holds each docket's folded
//! state for speed and can be rebuilt from `entries` at any time.

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

use crate::record::{DocketState, Stored, chain, hex};

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
CREATE TABLE IF NOT EXISTS keys (
  keyid TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  added_at TEXT NOT NULL
);
";

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
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
