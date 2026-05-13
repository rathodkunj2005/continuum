//! Insight graph persistence in LanceDB (`graph_nodes`, `graph_edges`).
//!
//! Uses the same `Store` connection bundle as the rest of the app — no second DB handle.

use std::collections::HashSet;
use std::sync::Arc;

use arrow_array::{
    builder::{BooleanBuilder, FixedSizeListBuilder, Float32Builder, Int64Builder, StringBuilder},
    Array, FixedSizeListArray, RecordBatch, RecordBatchIterator, RecordBatchReader,
};
use arrow_schema::{DataType, Field, Schema};
use chrono::Utc;
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::table::AddDataMode;
use uuid::Uuid;

use super::Store;
use crate::config::DEFAULT_TEXT_EMBEDDING_DIM;
use crate::memory::graph::schema::{
    GraphEdge, GraphEdgeType, GraphNode, GraphNodeType, GraphSubgraph,
};

fn escape_sql_literal(s: &str) -> String {
    s.replace('\'', "''")
}

const TEXT_DIM: i32 = DEFAULT_TEXT_EMBEDDING_DIM as i32;

fn node_type_lit(t: GraphNodeType) -> &'static str {
    match t {
        GraphNodeType::Project => "Project",
        GraphNodeType::Concept => "Concept",
        GraphNodeType::Decision => "Decision",
        GraphNodeType::File => "File",
        GraphNodeType::Error => "Error",
        GraphNodeType::Tool => "Tool",
        GraphNodeType::Person => "Person",
        GraphNodeType::Url => "Url",
        GraphNodeType::Session => "Session",
        GraphNodeType::Task => "Task",
    }
}

fn edge_type_lit(t: GraphEdgeType) -> &'static str {
    match t {
        GraphEdgeType::DependsOn => "DependsOn",
        GraphEdgeType::Contains => "Contains",
        GraphEdgeType::Imports => "Imports",
        GraphEdgeType::Extends => "Extends",
        GraphEdgeType::Implements => "Implements",
        GraphEdgeType::PartOf => "PartOf",
        GraphEdgeType::Supports => "Supports",
        GraphEdgeType::Contradicts => "Contradicts",
        GraphEdgeType::Supersedes => "Supersedes",
        GraphEdgeType::Refines => "Refines",
        GraphEdgeType::Questions => "Questions",
        GraphEdgeType::Resolves => "Resolves",
        GraphEdgeType::Causes => "Causes",
        GraphEdgeType::Prevents => "Prevents",
        GraphEdgeType::TriggeredBy => "TriggeredBy",
        GraphEdgeType::FixedBy => "FixedBy",
        GraphEdgeType::BrokeBy => "BrokeBy",
        GraphEdgeType::PrecededBy => "PrecededBy",
        GraphEdgeType::FollowedBy => "FollowedBy",
        GraphEdgeType::SimilarTo => "SimilarTo",
        GraphEdgeType::MentionedIn => "MentionedIn",
        GraphEdgeType::UsedIn => "UsedIn",
        GraphEdgeType::CreatedBy => "CreatedBy",
        GraphEdgeType::AppliesTo => "AppliesTo",
    }
}

fn parse_node_type(s: &str) -> GraphNodeType {
    match s {
        "Project" => GraphNodeType::Project,
        "Concept" => GraphNodeType::Concept,
        "Decision" => GraphNodeType::Decision,
        "File" => GraphNodeType::File,
        "Error" => GraphNodeType::Error,
        "Tool" => GraphNodeType::Tool,
        "Person" => GraphNodeType::Person,
        "Url" => GraphNodeType::Url,
        "Session" => GraphNodeType::Session,
        "Task" => GraphNodeType::Task,
        _ => GraphNodeType::Concept,
    }
}

fn parse_edge_type(s: &str) -> GraphEdgeType {
    match s {
        "DependsOn" => GraphEdgeType::DependsOn,
        "Contains" => GraphEdgeType::Contains,
        "Imports" => GraphEdgeType::Imports,
        "Extends" => GraphEdgeType::Extends,
        "Implements" => GraphEdgeType::Implements,
        "PartOf" => GraphEdgeType::PartOf,
        "Supports" => GraphEdgeType::Supports,
        "Contradicts" => GraphEdgeType::Contradicts,
        "Supersedes" => GraphEdgeType::Supersedes,
        "Refines" => GraphEdgeType::Refines,
        "Questions" => GraphEdgeType::Questions,
        "Resolves" => GraphEdgeType::Resolves,
        "Causes" => GraphEdgeType::Causes,
        "Prevents" => GraphEdgeType::Prevents,
        "TriggeredBy" => GraphEdgeType::TriggeredBy,
        "FixedBy" => GraphEdgeType::FixedBy,
        "BrokeBy" => GraphEdgeType::BrokeBy,
        "PrecededBy" => GraphEdgeType::PrecededBy,
        "FollowedBy" => GraphEdgeType::FollowedBy,
        "SimilarTo" => GraphEdgeType::SimilarTo,
        "MentionedIn" => GraphEdgeType::MentionedIn,
        "UsedIn" => GraphEdgeType::UsedIn,
        "CreatedBy" => GraphEdgeType::CreatedBy,
        "AppliesTo" => GraphEdgeType::AppliesTo,
        _ => GraphEdgeType::MentionedIn,
    }
}

fn zero_embedding() -> Vec<f32> {
    vec![0.0f32; DEFAULT_TEXT_EMBEDDING_DIM]
}

fn nodes_to_batch(nodes: &[GraphNode]) -> Result<RecordBatch, Box<dyn std::error::Error>> {
    let n = nodes.len();
    let mut id = StringBuilder::with_capacity(n, n * 40);
    let mut node_type = StringBuilder::with_capacity(n, n * 12);
    let mut label = StringBuilder::with_capacity(n, n * 80);
    let mut confidence = Float32Builder::with_capacity(n);
    let mut source_ids = StringBuilder::with_capacity(n, n * 64);
    let value_builder = Float32Builder::with_capacity(n * DEFAULT_TEXT_EMBEDDING_DIM);
    let mut emb_b = FixedSizeListBuilder::new(value_builder, TEXT_DIM);
    let mut created = Int64Builder::with_capacity(n);
    let mut updated = Int64Builder::with_capacity(n);
    let mut stale = BooleanBuilder::with_capacity(n);
    let mut metadata = StringBuilder::with_capacity(n, n * 64);

    for node in nodes {
        id.append_value(node.id.to_string());
        node_type.append_value(node_type_lit(node.node_type));
        label.append_value(&node.label);
        confidence.append_value(node.confidence);
        source_ids.append_value(serde_json::to_string(&node.source_memory_ids)?);
        let emb = node
            .embedding
            .as_ref()
            .cloned()
            .unwrap_or_else(zero_embedding);
        for v in emb.iter().take(DEFAULT_TEXT_EMBEDDING_DIM) {
            emb_b.values().append_value(*v);
        }
        emb_b.append(true);
        created.append_value(node.created_at.timestamp_millis());
        updated.append_value(node.updated_at.timestamp_millis());
        stale.append_value(node.stale);
        metadata.append_value(node.metadata.to_string());
    }

    let emb_arr = emb_b.finish();
    let schema = Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("node_type", DataType::Utf8, false),
        Field::new("label", DataType::Utf8, false),
        Field::new("confidence", DataType::Float32, false),
        Field::new("source_memory_ids", DataType::Utf8, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                TEXT_DIM,
            ),
            false,
        ),
        Field::new("created_at_ms", DataType::Int64, false),
        Field::new("updated_at_ms", DataType::Int64, false),
        Field::new("stale", DataType::Boolean, false),
        Field::new("metadata", DataType::Utf8, false),
    ]);
    let schema = Arc::new(schema);
    RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(id.finish()),
            Arc::new(node_type.finish()),
            Arc::new(label.finish()),
            Arc::new(confidence.finish()),
            Arc::new(source_ids.finish()),
            Arc::new(emb_arr),
            Arc::new(created.finish()),
            Arc::new(updated.finish()),
            Arc::new(stale.finish()),
            Arc::new(metadata.finish()),
        ],
    )
    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })
}

fn edges_to_batch(edges: &[GraphEdge]) -> Result<RecordBatch, Box<dyn std::error::Error>> {
    let n = edges.len();
    let mut id = StringBuilder::with_capacity(n, n * 40);
    let mut s = StringBuilder::with_capacity(n, n * 40);
    let mut t = StringBuilder::with_capacity(n, n * 40);
    let mut et = StringBuilder::with_capacity(n, n * 24);
    let mut conf = Float32Builder::with_capacity(n);
    let mut cf = BooleanBuilder::with_capacity(n);
    let mut created = Int64Builder::with_capacity(n);
    let mut meta = StringBuilder::with_capacity(n, n * 48);
    for e in edges {
        id.append_value(e.id.to_string());
        s.append_value(e.source_id.to_string());
        t.append_value(e.target_id.to_string());
        et.append_value(edge_type_lit(e.edge_type));
        conf.append_value(e.confidence);
        cf.append_value(e.conflict_flag);
        created.append_value(e.created_at.timestamp_millis());
        meta.append_value(e.metadata.to_string());
    }
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("source_id", DataType::Utf8, false),
        Field::new("target_id", DataType::Utf8, false),
        Field::new("edge_type", DataType::Utf8, false),
        Field::new("confidence", DataType::Float32, false),
        Field::new("conflict_flag", DataType::Boolean, false),
        Field::new("created_at_ms", DataType::Int64, false),
        Field::new("metadata", DataType::Utf8, false),
    ]));
    RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(id.finish()),
            Arc::new(s.finish()),
            Arc::new(t.finish()),
            Arc::new(et.finish()),
            Arc::new(conf.finish()),
            Arc::new(cf.finish()),
            Arc::new(created.finish()),
            Arc::new(meta.finish()),
        ],
    )
    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })
}

fn batch_to_nodes(batch: &RecordBatch) -> Result<Vec<GraphNode>, Box<dyn std::error::Error>> {
    let id = batch
        .column_by_name("id")
        .ok_or("missing id")?
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .ok_or("id type")?;
    let node_type = batch
        .column_by_name("node_type")
        .ok_or("missing node_type")?
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .ok_or("node_type type")?;
    let label = batch
        .column_by_name("label")
        .ok_or("missing label")?
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .ok_or("label type")?;
    let confidence = batch
        .column_by_name("confidence")
        .ok_or("missing confidence")?
        .as_any()
        .downcast_ref::<arrow_array::Float32Array>()
        .ok_or("confidence type")?;
    let source_ids = batch
        .column_by_name("source_memory_ids")
        .ok_or("missing source_memory_ids")?
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .ok_or("source_memory_ids type")?;
    let emb_col = batch
        .column_by_name("embedding")
        .ok_or("missing embedding")?
        .as_any()
        .downcast_ref::<FixedSizeListArray>()
        .ok_or("embedding type")?;
    let created = batch
        .column_by_name("created_at_ms")
        .ok_or("missing created_at_ms")?
        .as_any()
        .downcast_ref::<arrow_array::Int64Array>()
        .ok_or("created_at_ms type")?;
    let updated = batch
        .column_by_name("updated_at_ms")
        .ok_or("missing updated_at_ms")?
        .as_any()
        .downcast_ref::<arrow_array::Int64Array>()
        .ok_or("updated_at_ms type")?;
    let stale = batch
        .column_by_name("stale")
        .ok_or("missing stale")?
        .as_any()
        .downcast_ref::<arrow_array::BooleanArray>()
        .ok_or("stale type")?;
    let metadata = batch
        .column_by_name("metadata")
        .ok_or("missing metadata")?
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .ok_or("metadata type")?;

    let mut out = Vec::new();
    for i in 0..batch.num_rows() {
        let uid = Uuid::parse_str(id.value(i))?;
        let mut embedding = None;
        if !emb_col.is_null(i) {
            let flat = emb_col.value(i);
            let fa = flat
                .as_any()
                .downcast_ref::<arrow_array::Float32Array>()
                .ok_or("emb flat")?;
            let mut vec = Vec::with_capacity(DEFAULT_TEXT_EMBEDDING_DIM);
            for j in 0..fa.len() {
                vec.push(fa.value(j));
            }
            embedding = Some(vec);
        }
        let sm: Vec<String> = serde_json::from_str(source_ids.value(i))?;
        let meta: serde_json::Value = serde_json::from_str(metadata.value(i)).unwrap_or_default();
        out.push(GraphNode {
            id: uid,
            node_type: parse_node_type(node_type.value(i)),
            label: label.value(i).to_string(),
            confidence: confidence.value(i),
            source_memory_ids: sm,
            embedding,
            created_at: chrono::DateTime::from_timestamp_millis(created.value(i))
                .unwrap_or_else(Utc::now),
            updated_at: chrono::DateTime::from_timestamp_millis(updated.value(i))
                .unwrap_or_else(Utc::now),
            stale: stale.value(i),
            metadata: meta,
        });
    }
    Ok(out)
}

fn batch_to_edges(batch: &RecordBatch) -> Result<Vec<GraphEdge>, Box<dyn std::error::Error>> {
    let id = batch
        .column_by_name("id")
        .ok_or("missing id")?
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .ok_or("id type")?;
    let s = batch
        .column_by_name("source_id")
        .ok_or("missing source_id")?
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .ok_or("source_id type")?;
    let t = batch
        .column_by_name("target_id")
        .ok_or("missing target_id")?
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .ok_or("target_id type")?;
    let et = batch
        .column_by_name("edge_type")
        .ok_or("missing edge_type")?
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .ok_or("edge_type type")?;
    let conf = batch
        .column_by_name("confidence")
        .ok_or("missing confidence")?
        .as_any()
        .downcast_ref::<arrow_array::Float32Array>()
        .ok_or("confidence type")?;
    let cf = batch
        .column_by_name("conflict_flag")
        .ok_or("missing conflict_flag")?
        .as_any()
        .downcast_ref::<arrow_array::BooleanArray>()
        .ok_or("conflict_flag type")?;
    let created = batch
        .column_by_name("created_at_ms")
        .ok_or("missing created_at_ms")?
        .as_any()
        .downcast_ref::<arrow_array::Int64Array>()
        .ok_or("created_at_ms type")?;
    let meta = batch
        .column_by_name("metadata")
        .ok_or("missing metadata")?
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .ok_or("metadata type")?;

    let mut out = Vec::new();
    for i in 0..batch.num_rows() {
        let meta_val: serde_json::Value = serde_json::from_str(meta.value(i)).unwrap_or_default();
        out.push(GraphEdge {
            id: Uuid::parse_str(id.value(i))?,
            source_id: Uuid::parse_str(s.value(i))?,
            target_id: Uuid::parse_str(t.value(i))?,
            edge_type: parse_edge_type(et.value(i)),
            confidence: conf.value(i),
            conflict_flag: cf.value(i),
            created_at: chrono::DateTime::from_timestamp_millis(created.value(i))
                .unwrap_or_else(Utc::now),
            metadata: meta_val,
        });
    }
    Ok(out)
}

/// Insight graph Lance API (reuses `Store`'s opened `graph_*` tables).
pub struct GraphStore {
    store: Arc<Store>,
}

impl GraphStore {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    pub async fn all_nodes(&self) -> Result<Vec<GraphNode>, Box<dyn std::error::Error>> {
        let batches = self
            .store
            .graph_nodes_table
            .query()
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        let mut out = Vec::new();
        for b in batches {
            out.extend(batch_to_nodes(&b)?);
        }
        Ok(out)
    }

    pub async fn all_edges(&self) -> Result<Vec<GraphEdge>, Box<dyn std::error::Error>> {
        let batches = self
            .store
            .graph_edges_table
            .query()
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        let mut out = Vec::new();
        for b in batches {
            out.extend(batch_to_edges(&b)?);
        }
        Ok(out)
    }

    /// Merge by exact `id` (replace row). No fuzzy label merge.
    pub async fn upsert_node(&self, node: &GraphNode) -> Result<(), Box<dyn std::error::Error>> {
        let filter = format!("id = '{}'", escape_sql_literal(&node.id.to_string()));
        let _ = self.store.graph_nodes_table.delete(&filter).await;
        let batch = nodes_to_batch(std::slice::from_ref(node))?;
        let schema = batch.schema();
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.store
            .graph_nodes_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Append)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn upsert_edge(&self, edge: &GraphEdge) -> Result<(), Box<dyn std::error::Error>> {
        let existing = self.all_edges().await?;
        let dup = existing.iter().any(|e| {
            e.source_id == edge.source_id
                && e.target_id == edge.target_id
                && e.edge_type == edge.edge_type
        });
        if dup {
            return Ok(());
        }
        let filter = format!("id = '{}'", escape_sql_literal(&edge.id.to_string()));
        let _ = self.store.graph_edges_table.delete(&filter).await;
        let batch = edges_to_batch(std::slice::from_ref(edge))?;
        let schema = batch.schema();
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.store
            .graph_edges_table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Append)
            .execute()
            .await?;
        Ok(())
    }

    pub async fn get_subgraph(
        &self,
        start: Uuid,
        depth: u32,
    ) -> Result<GraphSubgraph, Box<dyn std::error::Error>> {
        let depth = depth.min(3);
        let nodes = self.all_nodes().await?;
        let edges = self.all_edges().await?;
        let sub = GraphSubgraph {
            nodes,
            edges,
            ..Default::default()
        };
        Ok(crate::memory::graph::traversal::bfs_neighborhood(
            &sub, start, depth,
        ))
    }

    pub async fn get_project_subgraph(
        &self,
        project_label: &str,
    ) -> Result<GraphSubgraph, Box<dyn std::error::Error>> {
        let nodes = self.all_nodes().await?;
        let edges = self.all_edges().await?;
        let seed: HashSet<_> = nodes
            .iter()
            .filter(|n| {
                n.node_type == GraphNodeType::Project && n.label.eq_ignore_ascii_case(project_label)
            })
            .map(|n| n.id)
            .collect();
        if seed.is_empty() {
            return Ok(GraphSubgraph::default());
        }
        let mut acc_n: HashSet<Uuid> = HashSet::new();
        for s in seed {
            let part = crate::memory::graph::traversal::bfs_neighborhood(
                &GraphSubgraph {
                    nodes: nodes.clone(),
                    edges: edges.clone(),
                    ..Default::default()
                },
                s,
                3,
            );
            for n in part.nodes {
                acc_n.insert(n.id);
            }
        }
        let nset = acc_n;
        let filt_nodes: Vec<_> = nodes.into_iter().filter(|n| nset.contains(&n.id)).collect();
        let idset: HashSet<_> = nset;
        let filt_edges: Vec<_> = edges
            .into_iter()
            .filter(|e| idset.contains(&e.source_id) && idset.contains(&e.target_id))
            .collect();
        Ok(GraphSubgraph {
            nodes: filt_nodes,
            edges: filt_edges,
            ..Default::default()
        })
    }

    pub async fn search_nodes(
        &self,
        query_embedding: &[f32],
        k: usize,
    ) -> Result<Vec<GraphNode>, Box<dyn std::error::Error>> {
        if query_embedding.len() != DEFAULT_TEXT_EMBEDDING_DIM {
            return Err("embedding dim mismatch".into());
        }
        let q = self.store.graph_nodes_table.query();
        let batches = q.execute().await?.try_collect::<Vec<_>>().await?;
        let mut nodes = Vec::new();
        for b in batches {
            nodes.extend(batch_to_nodes(&b)?);
        }
        let mut scored: Vec<(f32, GraphNode)> = Vec::new();
        let qn: f32 = query_embedding
            .iter()
            .map(|x| x * x)
            .sum::<f32>()
            .sqrt()
            .max(1e-6);
        for mut n in nodes {
            let emb = n.embedding.take().unwrap_or_else(zero_embedding);
            if emb.len() != query_embedding.len() {
                continue;
            }
            let dn: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
            let sim: f32 = emb
                .iter()
                .zip(query_embedding.iter())
                .map(|(a, b)| a * b)
                .sum::<f32>()
                / (dn * qn);
            n.embedding = Some(emb);
            scored.push((sim, n));
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k.max(1));
        Ok(scored.into_iter().map(|(_, n)| n).collect())
    }

    pub async fn get_node_by_label(
        &self,
        label: &str,
        node_type: GraphNodeType,
    ) -> Result<Option<GraphNode>, Box<dyn std::error::Error>> {
        let want = escape_sql_literal(label);
        let ty = escape_sql_literal(node_type_lit(node_type));
        let filter = format!("label = '{want}' AND node_type = '{ty}'");
        let batches = self
            .store
            .graph_nodes_table
            .query()
            .only_if(filter)
            .execute()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        for b in batches {
            let mut rows = batch_to_nodes(&b)?;
            if let Some(n) = rows.pop() {
                return Ok(Some(n));
            }
        }
        Ok(None)
    }

    pub async fn mark_stale(
        &self,
        older_than_days: i64,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let cutoff = Utc::now().timestamp_millis() - older_than_days * 86_400_000;
        let mut nodes = self.all_nodes().await?;
        let mut n_stale = 0usize;
        for n in &mut nodes {
            if n.updated_at.timestamp_millis() < cutoff && !n.stale {
                n.stale = true;
                n_stale += 1;
                self.upsert_node(n).await?;
            }
        }
        Ok(n_stale)
    }

    pub async fn delete_node(&self, id: Uuid) -> Result<(), Box<dyn std::error::Error>> {
        let filter = format!("id = '{}'", escape_sql_literal(&id.to_string()));
        self.store.graph_nodes_table.delete(&filter).await?;
        let edges = self.all_edges().await?;
        let to_del: Vec<String> = edges
            .iter()
            .filter(|e| e.source_id == id || e.target_id == id)
            .map(|e| e.id.to_string())
            .collect();
        if !to_del.is_empty() {
            let f = to_del
                .iter()
                .map(|id| format!("id = '{}'", escape_sql_literal(id)))
                .collect::<Vec<_>>()
                .join(" OR ");
            let _ = self.store.graph_edges_table.delete(&f).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn exact_id_merge_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let store = tokio::task::spawn_blocking(move || Arc::new(Store::new(&path).unwrap()))
            .await
            .unwrap();
        let gs = GraphStore::new(store.clone());
        let now = Utc::now();
        let id = Uuid::new_v4();
        let n1 = GraphNode {
            id,
            node_type: GraphNodeType::Concept,
            label: "Alpha topic".into(),
            confidence: 0.9,
            source_memory_ids: vec!["m1".into()],
            embedding: None,
            created_at: now,
            updated_at: now,
            stale: false,
            metadata: serde_json::json!({}),
        };
        gs.upsert_node(&n1).await.unwrap();
        let mut n2 = n1.clone();
        n2.confidence = 0.5;
        n2.source_memory_ids = vec!["m1".into(), "m2".into()];
        gs.upsert_node(&n2).await.unwrap();
        let got = gs
            .get_node_by_label("Alpha topic", GraphNodeType::Concept)
            .await
            .unwrap();
        assert!(got.is_some());
        assert_eq!(got.unwrap().confidence, 0.5);
    }

    #[tokio::test]
    async fn edge_dedup_and_contradiction_preserved() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let store = tokio::task::spawn_blocking(move || Arc::new(Store::new(&path).unwrap()))
            .await
            .unwrap();
        let gs = GraphStore::new(store);
        let now = Utc::now();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let e1 = GraphEdge {
            id: Uuid::new_v4(),
            source_id: a,
            target_id: b,
            edge_type: GraphEdgeType::Contradicts,
            confidence: 0.9,
            conflict_flag: true,
            created_at: now,
            metadata: serde_json::json!({}),
        };
        gs.upsert_edge(&e1).await.unwrap();
        gs.upsert_edge(&e1).await.unwrap();
        let edges = gs.all_edges().await.unwrap();
        assert_eq!(edges.len(), 1);
        assert!(edges[0].conflict_flag);
    }

    #[tokio::test]
    async fn stale_marking() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let store = tokio::task::spawn_blocking(move || Arc::new(Store::new(&path).unwrap()))
            .await
            .unwrap();
        let gs = GraphStore::new(store);
        let old = Utc::now() - chrono::Duration::days(400);
        let id = Uuid::new_v4();
        let n = GraphNode {
            id,
            node_type: GraphNodeType::File,
            label: "stale file node label ok".into(),
            confidence: 0.8,
            source_memory_ids: vec![],
            embedding: None,
            created_at: old,
            updated_at: old,
            stale: false,
            metadata: serde_json::json!({}),
        };
        gs.upsert_node(&n).await.unwrap();
        let n_marked = gs.mark_stale(30).await.unwrap();
        assert!(n_marked >= 1);
        let got = gs
            .get_node_by_label(&n.label, GraphNodeType::File)
            .await
            .unwrap()
            .unwrap();
        assert!(got.stale);
    }
}
