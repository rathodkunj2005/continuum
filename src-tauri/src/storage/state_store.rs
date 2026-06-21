//! LanceDB-backed key/value state storage for local app metadata.

use arrow_array::{Int64Array, RecordBatch, RecordBatchIterator, RecordBatchReader, StringArray};
use arrow_schema::{ArrowError, DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{Connection, Table};
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;

const TABLE_NAME: &str = "app_state";

enum StateCommand {
    Load {
        key: String,
        reply: mpsc::Sender<Result<Option<String>, String>>,
    },
    Save {
        key: String,
        payload: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Delete {
        key: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
}

/// Small local state store used by graph/tasks/meetings.
///
/// Runs LanceDB operations in a dedicated worker thread so callers can use
/// synchronous APIs without blocking the app runtime incorrectly.
pub struct StateStore {
    tx: mpsc::Sender<StateCommand>,
}

impl StateStore {
    pub fn new(data_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let db_path = data_dir.join("lancedb");
        std::fs::create_dir_all(&db_path)?;

        let init_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let table = init_rt.block_on(open_or_create_table(&db_path))?;

        let (tx, rx) = mpsc::channel::<StateCommand>();
        std::thread::Builder::new()
            .name("continuum-state-store".to_string())
            .spawn(move || run_worker(table, rx))?;

        Ok(Self { tx })
    }

    pub fn load_json<T>(&self, key: &str) -> Result<Option<T>, String>
    where
        T: DeserializeOwned,
    {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(StateCommand::Load {
                key: key.to_string(),
                reply: reply_tx,
            })
            .map_err(|_| "State store worker is unavailable".to_string())?;

        let maybe_payload = reply_rx
            .recv()
            .map_err(|_| "State store worker closed unexpectedly".to_string())??;

        match maybe_payload {
            Some(payload) => serde_json::from_str::<T>(&payload)
                .map(Some)
                .map_err(|e| format!("Failed parsing state key '{key}': {e}")),
            None => Ok(None),
        }
    }

    pub fn save_json<T>(&self, key: &str, value: &T) -> Result<(), String>
    where
        T: Serialize,
    {
        let payload = serde_json::to_string(value)
            .map_err(|e| format!("Failed serializing state key '{key}': {e}"))?;

        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(StateCommand::Save {
                key: key.to_string(),
                payload,
                reply: reply_tx,
            })
            .map_err(|_| "State store worker is unavailable".to_string())?;

        reply_rx
            .recv()
            .map_err(|_| "State store worker closed unexpectedly".to_string())?
    }

    pub fn delete_key(&self, key: &str) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(StateCommand::Delete {
                key: key.to_string(),
                reply: reply_tx,
            })
            .map_err(|_| "State store worker is unavailable".to_string())?;

        reply_rx
            .recv()
            .map_err(|_| "State store worker closed unexpectedly".to_string())?
    }
}

fn run_worker(table: Table, rx: mpsc::Receiver<StateCommand>) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            let msg = format!("Failed to initialize state store runtime: {err}");
            while let Ok(command) = rx.recv() {
                match command {
                    StateCommand::Load { reply, .. } => {
                        let _ = reply.send(Err(msg.clone()));
                    }
                    StateCommand::Save { reply, .. } | StateCommand::Delete { reply, .. } => {
                        let _ = reply.send(Err(msg.clone()));
                    }
                }
            }
            return;
        }
    };

    while let Ok(command) = rx.recv() {
        match command {
            StateCommand::Load { key, reply } => {
                let result = runtime.block_on(load_payload(&table, &key));
                let _ = reply.send(result);
            }
            StateCommand::Save {
                key,
                payload,
                reply,
            } => {
                let result = runtime.block_on(save_payload(&table, &key, &payload));
                let _ = reply.send(result);
            }
            StateCommand::Delete { key, reply } => {
                let result = runtime.block_on(delete_payload(&table, &key));
                let _ = reply.send(result);
            }
        }
    }
}

async fn load_payload(table: &Table, key: &str) -> Result<Option<String>, String> {
    let escaped = sql_escape(key);
    let batches: Vec<RecordBatch> = table
        .query()
        .only_if(format!("state_key = '{escaped}'"))
        .limit(1)
        .execute()
        .await
        .map_err(|e| format!("State load query failed: {e}"))?
        .try_collect()
        .await
        .map_err(|e| format!("State load collection failed: {e}"))?;

    for batch in &batches {
        let Some(key_col) = batch
            .column_by_name("state_key")
            .and_then(|col| col.as_any().downcast_ref::<StringArray>())
        else {
            continue;
        };
        let Some(payload_col) = batch
            .column_by_name("payload")
            .and_then(|col| col.as_any().downcast_ref::<StringArray>())
        else {
            continue;
        };

        for i in 0..batch.num_rows() {
            if key_col.value(i) == key {
                return Ok(Some(payload_col.value(i).to_string()));
            }
        }
    }

    Ok(None)
}

async fn save_payload(table: &Table, key: &str, payload: &str) -> Result<(), String> {
    let escaped = sql_escape(key);
    table
        .delete(&format!("state_key = '{escaped}'"))
        .await
        .map_err(|e| format!("State delete-before-save failed: {e}"))?;

    let batch = state_rows_to_batch(&[(
        key.to_string(),
        payload.to_string(),
        chrono::Utc::now().timestamp_millis(),
    )])
    .map_err(|e| format!("Failed to build state batch: {e}"))?;
    let schema = Arc::new(state_schema());
    let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);

    table
        .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
        .execute()
        .await
        .map_err(|e| format!("State save failed: {e}"))?;
    Ok(())
}

async fn delete_payload(table: &Table, key: &str) -> Result<(), String> {
    let escaped = sql_escape(key);
    table
        .delete(&format!("state_key = '{escaped}'"))
        .await
        .map(|_| ())
        .map_err(|e| format!("State delete failed: {e}"))
}

fn state_schema() -> Schema {
    Schema::new(vec![
        Field::new("state_key", DataType::Utf8, false),
        Field::new("payload", DataType::Utf8, false),
        Field::new("updated_at", DataType::Int64, false),
    ])
}

fn state_rows_to_batch(rows: &[(String, String, i64)]) -> Result<RecordBatch, ArrowError> {
    let keys = rows.iter().map(|(k, _, _)| k.clone()).collect::<Vec<_>>();
    let payloads = rows.iter().map(|(_, p, _)| p.clone()).collect::<Vec<_>>();
    let updated = rows.iter().map(|(_, _, ts)| *ts).collect::<Vec<_>>();

    RecordBatch::try_new(
        Arc::new(state_schema()),
        vec![
            Arc::new(StringArray::from(keys)),
            Arc::new(StringArray::from(payloads)),
            Arc::new(Int64Array::from(updated)),
        ],
    )
}

fn sql_escape(value: &str) -> String {
    value.replace('\'', "''")
}

async fn open_or_create_table(db_path: &Path) -> Result<Table, lancedb::Error> {
    let uri = db_path.to_string_lossy();
    let conn: Connection = lancedb::connect(&uri).execute().await?;

    let names = conn.table_names().execute().await?;
    if names.contains(&TABLE_NAME.to_string()) {
        conn.open_table(TABLE_NAME).execute().await
    } else {
        let schema = Arc::new(state_schema());
        let empty = RecordBatchIterator::new(
            std::iter::empty::<Result<RecordBatch, ArrowError>>(),
            schema,
        );
        conn.create_table(
            TABLE_NAME,
            Box::new(empty) as Box<dyn RecordBatchReader + Send>,
        )
        .execute()
        .await
    }
}
