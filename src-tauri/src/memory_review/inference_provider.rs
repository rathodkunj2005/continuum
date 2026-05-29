//! Production [`ReviewProvider`] backed by [`InferenceEngine`]. Tests live in
//! `pipeline.rs` and use a deterministic stub instead.

use crate::inference::{InferenceEngine, MemoryReviewCandidate, MemoryReviewPromptInput};
use futures::future::{BoxFuture, FutureExt};
use std::sync::Arc;

use super::pipeline::{ReviewInput, ReviewProvider, ReviewedMemory};

pub struct InferenceReviewProvider {
    engine: Arc<InferenceEngine>,
}

impl InferenceReviewProvider {
    pub fn new(engine: Arc<InferenceEngine>) -> Self {
        Self { engine }
    }
}

impl ReviewProvider for InferenceReviewProvider {
    fn review<'a>(
        &'a self,
        input: &'a ReviewInput,
    ) -> BoxFuture<'a, Result<ReviewedMemory, String>> {
        async move {
            let prompt_input = MemoryReviewPromptInput {
                memory_id: input.memory_id.clone(),
                app_name: input.app_name.clone(),
                window_title: input.window_title.clone(),
                url: input.url.clone(),
                clean_text: input.clean_text.clone(),
                current_memory_context: input.current_memory_context.clone(),
                current_display_summary: input.current_display_summary.clone(),
                synthesis_branch: input.synthesis_branch.clone(),
                same_day_candidates: input
                    .same_day_candidates
                    .iter()
                    .map(|c| MemoryReviewCandidate {
                        id: c.id.clone(),
                        display_title: c.display_title.clone(),
                    })
                    .collect(),
            };
            match self.engine.review_memory_record(&prompt_input).await {
                Some(parsed) => Ok(ReviewedMemory {
                    memory_context: parsed.memory_context,
                    display_summary: parsed.display_summary,
                    topic: parsed.topic,
                    user_intent: parsed.user_intent,
                    activity_type: parsed.activity_type,
                    related_memory_ids: parsed.related_memory_ids,
                    confidence: parsed.confidence,
                }),
                None => {
                    Err("inference: review_memory_record returned no parseable JSON".to_string())
                }
            }
        }
        .boxed()
    }
}
