//! 診断キャッシュと履歴（SQLite）。
//!
//! - 診断は 1 画像につき 1 回。結果は画像の中身のハッシュで引く
//! - 再診断しても前の結果は消さず、新しい行を追加して `is_latest` を付け替える
//! - 履歴はアプリのデータフォルダに残す（Undo 履歴と違い、再起動後も保持する）

use super::Suggestion;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("診断キャッシュのデータベースを扱えません: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("診断キャッシュの内容を解釈できません: {0}")]
    Json(#[from] serde_json::Error),
    #[error("診断キャッシュのフォルダを作れません: {0}")]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, StoreError>;

/// 保存されている診断 1 回ぶん
#[derive(Debug, Clone, PartialEq)]
pub struct StoredDiagnosis {
    pub id: i64,
    pub image_hash: String,
    pub diagnosed_at: String,
    pub backend: String,
    pub model: String,
    /// 診断時に渡した選択肢（振り分け先の識別子）。追加されたフォルダが未評価かの判定に使う
    pub choices: Vec<String>,
    pub suggestion: Suggestion,
    pub is_latest: bool,
}

pub struct Store {
    conn: Connection,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS diagnoses (
  id            INTEGER PRIMARY KEY,
  image_hash    TEXT NOT NULL,
  diagnosed_at  TEXT NOT NULL,
  backend       TEXT NOT NULL,
  model         TEXT NOT NULL,
  choices_json  TEXT NOT NULL,
  scores_json   TEXT NOT NULL,
  is_latest     INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_diag_hash ON diagnoses(image_hash, backend, is_latest);
";

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }

    /// 診断を 1 行追加し、同じ画像・同じバックエンドの前の「最新」を外す（古い行は残す）
    pub fn record_diagnosis(
        &mut self,
        image_hash: &str,
        backend: &str,
        model: &str,
        choices: &[String],
        suggestion: &Suggestion,
        at: &str,
    ) -> Result<i64> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE diagnoses SET is_latest = 0 WHERE image_hash = ?1 AND backend = ?2 AND is_latest = 1",
            params![image_hash, backend],
        )?;
        tx.execute(
            "INSERT INTO diagnoses (image_hash, diagnosed_at, backend, model, choices_json, scores_json, is_latest)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
            params![
                image_hash,
                at,
                backend,
                model,
                serde_json::to_string(choices)?,
                serde_json::to_string(suggestion)?
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    /// 最新の診断。モデルを更新しても自動では再診断しないので、モデル名では絞らない
    pub fn latest_diagnosis(&self, image_hash: &str, backend: &str) -> Result<Option<StoredDiagnosis>> {
        self.conn
            .query_row(DIAG_SELECT_LATEST, params![image_hash, backend], row_to_raw)
            .optional()?
            .map(raw_to_diagnosis)
            .transpose()
    }

    /// 1 画像の診断履歴（新しい順）
    pub fn diagnosis_history(&self, image_hash: &str) -> Result<Vec<StoredDiagnosis>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, image_hash, diagnosed_at, backend, model, choices_json, scores_json, is_latest
             FROM diagnoses WHERE image_hash = ?1 ORDER BY id DESC",
        )?;
        let rows = stmt.query_map(params![image_hash], row_to_raw)?.collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter().map(raw_to_diagnosis).collect()
    }
}

const DIAG_SELECT_LATEST: &str =
    "SELECT id, image_hash, diagnosed_at, backend, model, choices_json, scores_json, is_latest
     FROM diagnoses WHERE image_hash = ?1 AND backend = ?2 AND is_latest = 1";

type RawRow = (i64, String, String, String, String, String, String, i64);

fn row_to_raw(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?))
}

fn raw_to_diagnosis(r: RawRow) -> Result<StoredDiagnosis> {
    Ok(StoredDiagnosis {
        id: r.0,
        image_hash: r.1,
        diagnosed_at: r.2,
        backend: r.3,
        model: r.4,
        choices: serde_json::from_str(&r.5)?,
        suggestion: serde_json::from_str(&r.6)?,
        is_latest: r.7 != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::{ScoreKind, Scored};

    fn suggestion(scores: &[(&str, f32)]) -> Suggestion {
        Suggestion {
            kind: ScoreKind::Probability,
            scores: scores.iter().map(|(i, s)| Scored { id: (*i).into(), score: *s }).collect(),
            none_of_above: Some(0.1),
        }
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_recorded_diagnosis_can_be_read_back_by_hash() {
        let mut s = Store::open_in_memory().unwrap();
        assert!(s.latest_diagnosis("h1", "systemone").unwrap().is_none());
        let sg = suggestion(&[("a", 0.6), ("b", 0.3)]);
        s.record_diagnosis("h1", "systemone", "clef-flash", &ids(&["a", "b"]), &sg, "2026-10-03T10:00:00Z").unwrap();

        let d = s.latest_diagnosis("h1", "systemone").unwrap().unwrap();
        assert_eq!(d.suggestion, sg);
        assert_eq!(d.choices, ids(&["a", "b"]));
        assert_eq!((d.model.as_str(), d.diagnosed_at.as_str()), ("clef-flash", "2026-10-03T10:00:00Z"));
        assert!(d.is_latest);
        assert!(s.latest_diagnosis("h2", "systemone").unwrap().is_none(), "別の画像は別");
        assert!(s.latest_diagnosis("h1", "local").unwrap().is_none(), "バックエンドごとに持つ");
    }

    #[test]
    fn rediagnosis_adds_a_row_and_moves_the_latest_mark_without_deleting_history() {
        let mut s = Store::open_in_memory().unwrap();
        let old = suggestion(&[("a", 0.7)]);
        let new = suggestion(&[("a", 0.2), ("b", 0.7)]);
        let id1 = s.record_diagnosis("h", "systemone", "m1", &ids(&["a"]), &old, "t1").unwrap();
        let id2 = s.record_diagnosis("h", "systemone", "m2", &ids(&["a", "b"]), &new, "t2").unwrap();
        assert_ne!(id1, id2);

        let latest = s.latest_diagnosis("h", "systemone").unwrap().unwrap();
        assert_eq!((latest.id, &latest.suggestion), (id2, &new));

        let history = s.diagnosis_history("h").unwrap();
        assert_eq!(history.len(), 2, "前の結果は消さない");
        assert_eq!((history[0].id, history[0].is_latest), (id2, true), "新しい順");
        assert_eq!((history[1].id, history[1].is_latest), (id1, false));
        assert_eq!(history[1].suggestion, old);
    }

    #[test]
    fn latest_marks_are_independent_per_backend() {
        let mut s = Store::open_in_memory().unwrap();
        s.record_diagnosis("h", "local", "m", &ids(&["a"]), &suggestion(&[("a", 0.5)]), "t1").unwrap();
        s.record_diagnosis("h", "systemone", "m", &ids(&["a"]), &suggestion(&[("a", 0.9)]), "t2").unwrap();
        assert!(s.latest_diagnosis("h", "local").unwrap().is_some(), "別バックエンドの診断で外れない");
        assert!(s.latest_diagnosis("h", "systemone").unwrap().is_some());
    }

    #[test]
    fn data_survives_reopening_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/suggest.sqlite");
        let sg = suggestion(&[("a", 0.6)]);
        Store::open(&path).unwrap().record_diagnosis("h", "local", "m", &ids(&["a"]), &sg, "t").unwrap();
        let again = Store::open(&path).unwrap();
        assert_eq!(again.latest_diagnosis("h", "local").unwrap().unwrap().suggestion, sg);
    }
}
