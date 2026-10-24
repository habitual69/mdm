use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::types::Value as Sql;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, TransactionBehavior};

use crate::error::{Error, Result};
use crate::model::{
    dt_from_millis, EngineKind, Job, JobKind, JobPatch, JobState, now_millis,
};

pub struct Db {
    conn: Mutex<Connection>,
}

const COLUMNS: &str = "id, parent_id, url, kind, engine, state, title, output_dir, output_path, \
     format, progress, downloaded_bytes, total_bytes, speed_bps, upload_speed_bps, eta_sec, external_id, \
     error, retries, retry_at, created_at, updated_at, select_files";

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        // Tiny migration: add select_files when upgrading an older queue db.
        let has_select: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('jobs') WHERE name='select_files'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(1);
        if has_select == 0 {
            let _ = conn.execute("ALTER TABLE jobs ADD COLUMN select_files TEXT", []);
        }
        let has_upload: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('jobs') WHERE name='upload_speed_bps'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(1);
        if has_upload == 0 {
            let _ = conn.execute("ALTER TABLE jobs ADD COLUMN upload_speed_bps INTEGER", []);
        }
        Ok(Db { conn: Mutex::new(conn) })
    }

    pub fn open_default() -> Result<Db> {
        let dir = crate::config::data_dir();
        Db::open(&dir.join("queue.sqlite3"))
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let conn = self.conn.lock().expect("db lock poisoned");
        f(&conn).map_err(Error::from)
    }

    fn with_tx<T>(&self, f: impl FnOnce(&rusqlite::Transaction) -> rusqlite::Result<T>) -> Result<T> {
        let mut conn = self.conn.lock().expect("db lock poisoned");
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Error::from)?;
        let out = f(&tx).map_err(Error::from)?;
        tx.commit().map_err(Error::from)?;
        Ok(out)
    }

    pub fn insert_job(&self, job: &Job) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO jobs (id, parent_id, url, kind, engine, state, title, output_dir, \
                 output_path, format, progress, downloaded_bytes, total_bytes, speed_bps, upload_speed_bps, eta_sec, \
                 external_id, error, retries, retry_at, created_at, updated_at, select_files) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
                params![
                    job.id,
                    job.parent_id,
                    job.url,
                    job.kind.as_str(),
                    job.engine.as_str(),
                    job.state.as_str(),
                    job.title,
                    job.output_dir.display().to_string(),
                    job.output_path.as_ref().map(|p| p.display().to_string()),
                    job.format,
                    job.progress as f64,
                    job.downloaded_bytes,
                    job.total_bytes,
                    job.speed_bps,
                    job.upload_speed_bps,
                    job.eta_sec,
                    job.external_id,
                    job.error,
                    job.retries,
                    job.retry_at,
                    job.created_at.timestamp_millis(),
                    job.updated_at.timestamp_millis(),
                    job.select_files.as_ref().map(|l| serde_json::to_string(l).unwrap_or_default()),
                ],
            )
            .map(|_n| ())
        })
    }

    fn row_to_job(r: &rusqlite::Row) -> rusqlite::Result<Job> {
        let kind: String = r.get("kind")?;
        let engine: String = r.get("engine")?;
        let state: String = r.get("state")?;
        let out_dir: String = r.get("output_dir")?;
        let out_path: Option<String> = r.get("output_path")?;
        let created: i64 = r.get("created_at")?;
        let updated: i64 = r.get("updated_at")?;
        Ok(Job {
            id: r.get("id")?,
            parent_id: r.get("parent_id")?,
            url: r.get("url")?,
            kind: JobKind::parse(&kind).unwrap_or(JobKind::Video),
            engine: engine.parse_or_default(),
            state: JobState::parse(&state).unwrap_or(JobState::Failed),
            title: r.get("title")?,
            output_dir: PathBuf::from(out_dir),
            output_path: out_path.map(PathBuf::from),
            format: r.get("format")?,
            progress: r.get::<_, f64>("progress")? as f32,
            downloaded_bytes: r.get("downloaded_bytes")?,
            total_bytes: r.get("total_bytes")?,
            speed_bps: r.get("speed_bps")?,
            upload_speed_bps: r.get("upload_speed_bps").ok().flatten(),
            eta_sec: r.get("eta_sec")?,
            external_id: r.get("external_id")?,
            error: r.get("error")?,
            retries: r.get("retries")?,
            retry_at: r.get("retry_at")?,
            created_at: dt_from_millis(created),
            updated_at: dt_from_millis(updated),
            select_files: r
                .get::<_, Option<String>>("select_files")?
                .and_then(|s| serde_json::from_str::<Vec<i64>>(&s).ok()),
        })
    }

    pub fn get_job(&self, id: &str) -> Result<Option<Job>> {
        self.with(|c| {
            c.query_row(
                &format!("SELECT {COLUMNS} FROM jobs WHERE id = ?1"),
                params![id],
                Self::row_to_job,
            )
            .optional()
        })
    }

    pub fn list_jobs(&self, states: Option<&[JobState]>) -> Result<Vec<Job>> {
        self.with(|c| {
            let (where_sql, values): (String, Vec<Sql>) = match states {
                None => (String::new(), Vec::new()),
                Some([]) => (String::new(), Vec::new()),
                Some(list) => {
                    let marks: Vec<&str> = list.iter().map(|_| "?").collect();
                    (
                        format!(" WHERE state IN ({})", marks.join(",")),
                        list.iter().map(|s| Sql::Text(s.as_str().into())).collect(),
                    )
                }
            };
            let sql = format!(
                "SELECT {COLUMNS} FROM jobs{} ORDER BY created_at DESC, rowid DESC",
                where_sql
            );
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt
                .query_map(params_from_iter(values.iter()), Self::row_to_job)?
                .collect::<rusqlite::Result<Vec<Job>>>()?;
            Ok(rows)
        })
    }

    /// Atomically claim the oldest eligible queued job and mark it running.
    pub fn claim_next_queued(&self) -> Result<Option<Job>> {
        let now = now_millis();
        self.with_tx(|tx| {
            let job = tx
                .query_row(
                    &format!(
                        "SELECT {COLUMNS} FROM jobs WHERE state = 'queued' \
                         AND (retry_at IS NULL OR retry_at <= ?1) \
                         ORDER BY created_at ASC, rowid ASC LIMIT 1"
                    ),
                    params![now],
                    Self::row_to_job,
                )
                .optional()?;
            if let Some(mut j) = job {
                let now = now_millis();
                tx.execute(
                    "UPDATE jobs SET state = 'running', retry_at = NULL, updated_at = ?2 WHERE id = ?1",
                    params![j.id, now],
                )?;
                j.state = JobState::Running;
                j.updated_at = dt_from_millis(now);
                Ok(Some(j))
            } else {
                Ok(None)
            }
        })
    }

    pub fn count_running(&self) -> Result<i64> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM jobs WHERE state = 'running'",
                [],
                |r| r.get(0),
            )
        })
    }

    pub fn count_queued(&self) -> Result<i64> {
        self.with(|c| {
            c.query_row("SELECT COUNT(*) FROM jobs WHERE state = 'queued'", [], |r| r.get(0))
        })
    }

    pub fn apply_patch(&self, patch: &JobPatch) -> Result<Option<Job>> {
        let id = patch.id.clone();
        let patched_job = self.with(|c| {
            let mut sets: Vec<String> = Vec::new();
            let mut values: Vec<Sql> = Vec::new();
            let mut push = |col: &str, v: Sql, sets: &mut Vec<String>| {
                values.push(v);
                sets.push(format!("{} = ?{}", col, values.len()));
            };

            if let Some(s) = patch.state {
                push("state", Sql::Text(s.as_str().into()), &mut sets);
            }
            if let Some(t) = patch.title.clone() {
                push("title", Sql::Text(t), &mut sets);
            }
            if let Some(p) = patch.progress {
                push("progress", Sql::Real(p as f64), &mut sets);
            }
            if let Some(v) = patch.downloaded_bytes {
                push("downloaded_bytes", Sql::Integer(v), &mut sets);
            }
            if let Some(v) = patch.total_bytes {
                push("total_bytes", Sql::Integer(v), &mut sets);
            }
            if let Some(v) = patch.speed_bps {
                push("speed_bps", Sql::Integer(v), &mut sets);
            }
            if let Some(v) = patch.upload_speed_bps {
                push("upload_speed_bps", Sql::Integer(v), &mut sets);
            }
            if let Some(v) = patch.eta_sec {
                push("eta_sec", Sql::Integer(v), &mut sets);
            }
            if let Some(v) = patch.external_id.clone() {
                push("external_id", Sql::Text(v), &mut sets);
            }
            if let Some(v) = patch.error.clone() {
                push("error", Sql::Text(v), &mut sets);
            }
            if patch.clear_error {
                push("error", Sql::Null, &mut sets);
            }
            if let Some(v) = patch.output_path.clone() {
                push("output_path", Sql::Text(v.display().to_string()), &mut sets);
            }
            if let Some(v) = patch.retries {
                push("retries", Sql::Integer(v), &mut sets);
            }
            if patch.bump_retries {
                sets.push("retries = retries + 1".into());
            }
            if let Some(v) = patch.retry_at {
                match v {
                    Some(ts) => push("retry_at", Sql::Integer(ts), &mut sets),
                    None => push("retry_at", Sql::Null, &mut sets),
                }
            }

            if sets.is_empty() {
                return Ok(None);
            }
            values.push(Sql::Integer(patch.updated_at.unwrap_or_else(now_millis)));
            values.push(Sql::Text(id.clone()));
            let sql = format!(
                "UPDATE jobs SET {}, updated_at = ?{} WHERE id = ?{}",
                sets.join(", "),
                values.len() - 1,
                values.len()
            );
            let n = c.execute(&sql, params_from_iter(values.iter()))?;
            Ok((n > 0).then_some(()))
        })?;
        if patched_job.is_some() {
            self.get_job(&id)
        } else {
            Ok(None)
        }
    }

    pub fn delete_job(&self, id: &str) -> Result<bool> {
        self.with(|c| {
            let n = c.execute("DELETE FROM jobs WHERE id = ?1", params![id])?;
            Ok(n > 0)
        })
    }

    /// At daemon startup: rows left in Running/PostProcess from a previous
    /// session go back to Queued so they resume.
    pub fn reset_stale_running(&self) -> Result<usize> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE jobs SET state = 'queued', retry_at = NULL, updated_at = ?1 \
                 WHERE state IN ('running')",
                params![now_millis()],
            )?;
            Ok(n)
        })
    }

    pub fn ids_with_state(&self, state: JobState) -> Result<Vec<String>> {
        self.list_jobs(Some(&[state])).map(|v| v.into_iter().map(|j| j.id).collect())
    }
}

trait ParseEngineK {
    fn parse_or_default(&self) -> EngineKind;
}
impl ParseEngineK for String {
    fn parse_or_default(&self) -> EngineKind {
        if self == EngineKind::Aria2.as_str() {
            EngineKind::Aria2
        } else {
            EngineKind::Ytdlp
        }
    }
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY,
    parent_id TEXT,
    url TEXT NOT NULL,
    kind TEXT NOT NULL,
    engine TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'queued',
    title TEXT,
    output_dir TEXT NOT NULL,
    output_path TEXT,
    format TEXT NOT NULL DEFAULT 'best',
    progress REAL NOT NULL DEFAULT 0,
    downloaded_bytes INTEGER,
    total_bytes INTEGER,
    speed_bps INTEGER,
    upload_speed_bps INTEGER,
    eta_sec INTEGER,
    external_id TEXT,
    error TEXT,
    retries INTEGER NOT NULL DEFAULT 0,
    retry_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    select_files TEXT
);
CREATE INDEX IF NOT EXISTS jobs_state_idx ON jobs(state);
CREATE INDEX IF NOT EXISTS jobs_created_idx ON jobs(created_at);
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_db(tag: &str) -> Db {
        let mut name = format!("mdm_test_{}_", tag);
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        name.push_str(&seed.to_string());
        Db::open(&std::env::temp_dir().join(name)).unwrap()
    }

    #[test]
    fn insert_get_list_roundtrip() {
        let db = tmp_db("roundtrip");
        let job = Job::new(
            "https://example.com/video",
            JobKind::Video,
            PathBuf::from("/tmp/mdl"),
            "best".into(),
        );
        db.insert_job(&job).unwrap();
        let got = db.get_job(&job.id).unwrap().unwrap();
        assert_eq!(got.url, job.url);
        assert_eq!(got.kind, job.kind);
        assert_eq!(got.state, JobState::Queued);
        let list = db.list_jobs(None).unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn claim_and_patch_states() {
        let db = tmp_db("claim");
        let a = Job::new("a", JobKind::Direct, PathBuf::from("/tmp"), "best".into());
        let b = Job::new("b", JobKind::Video, PathBuf::from("/tmp"), "best".into());
        db.insert_job(&a).unwrap();
        db.insert_job(&b).unwrap();

        let claimed = db.claim_next_queued().unwrap().unwrap();
        assert_eq!(claimed.id, a.id); // created first unless identical millis; rowid tiebreak
        assert_eq!(claimed.state, JobState::Running);

        let mut patch = JobPatch::with_state(JobState::Paused);
        patch.id = claimed.id.clone();
        patch.progress = Some(0.5);
        let updated = db.apply_patch(&patch).unwrap().unwrap();
        assert_eq!(updated.state, JobState::Paused);
        assert!((updated.progress - 0.5).abs() < 1e-6);

        // b is still queued -> second claim returns it.
        let claimed_b = db.claim_next_queued().unwrap().unwrap();
        assert_eq!(claimed_b.id, b.id);
        assert_eq!(claimed_b.state, JobState::Running);
        assert!(db.claim_next_queued().unwrap().is_none());
    }

    #[test]
    fn retry_at_is_respected() {
        let db = tmp_db("retry");
        let a = Job::new("a", JobKind::Direct, PathBuf::from("/tmp"), "best".into());
        db.insert_job(&a).unwrap();
        // Future retry_at: not claimable.
        let mut patch = JobPatch::with_state(JobState::Queued);
        patch.id = a.id.clone();
        patch.retry_at = Some(Some(now_millis() + 10_000));
        db.apply_patch(&patch).unwrap();
        assert!(db.claim_next_queued().unwrap().is_none());
        // Queued without retry_at: claimable.
        let mut patch2 = JobPatch::with_state(JobState::Queued);
        patch2.id = a.id.clone();
        patch2.retry_at = Some(None);
        db.apply_patch(&patch2).unwrap();
        assert!(db.claim_next_queued().unwrap().is_some());
    }
}
