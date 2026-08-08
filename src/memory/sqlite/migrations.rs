//! Transactional schema upgrades for SQLite semantic embeddings.

use super::encode_vector;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};

const CREATE_TABLE: &str = r#"
    CREATE TABLE memory_embeddings (
        record_id TEXT NOT NULL,
        space_id TEXT NOT NULL,
        provider TEXT NOT NULL,
        model TEXT NOT NULL,
        revision TEXT NOT NULL,
        normalization TEXT NOT NULL,
        content_hash TEXT NOT NULL,
        dimension INTEGER NOT NULL,
        vector_blob BLOB NOT NULL,
        updated_at TEXT NOT NULL,
        PRIMARY KEY (record_id, space_id),
        FOREIGN KEY (record_id) REFERENCES memory(id)
    );
"#;

pub(super) fn ensure_embedding_schema(conn: &mut Connection) -> Result<()> {
    if !table_exists(conn, "memory_embeddings")? {
        conn.execute_batch(CREATE_TABLE)?;
        create_index(conn)?;
        return Ok(());
    }

    let columns = table_columns(conn, "memory_embeddings")?;
    let desired = [
        "record_id",
        "space_id",
        "provider",
        "model",
        "revision",
        "normalization",
        "content_hash",
        "dimension",
        "vector_blob",
        "updated_at",
    ];
    if desired
        .iter()
        .all(|column| columns.iter().any(|found| found == column))
    {
        create_index(conn)?;
        return Ok(());
    }

    let branch_schema = [
        "memory_id",
        "space_id",
        "content_hash",
        "dimension",
        "vector_json",
        "updated_at",
    ];
    if !branch_schema
        .iter()
        .all(|column| columns.iter().any(|found| found == column))
    {
        bail!(
            "unsupported memory_embeddings schema; found columns: {}",
            columns.join(", ")
        );
    }

    migrate_vector_json(conn)
}

fn migrate_vector_json(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute_batch("ALTER TABLE memory_embeddings RENAME TO memory_embeddings_legacy;")?;
    tx.execute_batch(CREATE_TABLE)?;
    let rows = {
        let mut statement = tx.prepare(
            "SELECT memory_id, space_id, content_hash, dimension, vector_json, updated_at
             FROM memory_embeddings_legacy",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (record_id, space_id, content_hash, dimension, vector_json, updated_at) in rows {
        let dimension =
            usize::try_from(dimension).context("negative legacy embedding dimension")?;
        let vector: Vec<f32> =
            serde_json::from_str(&vector_json).context("decode legacy JSON embedding vector")?;
        if vector.len() != dimension || !vector.iter().all(|value| value.is_finite()) {
            bail!("legacy embedding vector does not match its declared dimension");
        }
        tx.execute(
            "INSERT INTO memory_embeddings (
                record_id, space_id, provider, model, revision, normalization,
                content_hash, dimension, vector_blob, updated_at
             ) VALUES (?1,?2,'legacy-unknown','legacy-unknown','legacy-unknown',
                       'legacy-unknown',?3,?4,?5,?6)",
            params![
                record_id,
                space_id,
                content_hash,
                dimension as i64,
                encode_vector(&vector),
                updated_at
            ],
        )?;
    }
    tx.execute_batch(
        "DROP TABLE memory_embeddings_legacy;
         CREATE INDEX idx_memory_embeddings_space ON memory_embeddings(space_id);",
    )?;
    tx.commit()?;
    Ok(())
}

fn create_index(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_memory_embeddings_space
         ON memory_embeddings(space_id)",
        [],
    )?;
    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |row| row.get(0),
    )?)
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    Ok(columns.collect::<rusqlite::Result<Vec<_>>>()?)
}
