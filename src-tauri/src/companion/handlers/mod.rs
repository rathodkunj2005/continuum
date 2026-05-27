//! HTTP handlers for the Companion API v1.

pub mod ask;
pub mod capture;
pub mod feedback;
pub mod memories;
pub mod pairing;
pub mod search;
pub mod status;

use crate::companion::dto::CompanionMemoryCard;

pub fn companion_card_from_memory_card(card: crate::search::MemoryCard) -> CompanionMemoryCard {
    CompanionMemoryCard {
        memory_id: card.id,
        title: card.title,
        summary: card.summary,
        display_summary: card.display_summary,
        internal_context: card.internal_context,
        timestamp: card.timestamp,
        app_name: card.app_name,
        window_title: card.window_title,
        url: card.url,
        score: card.score,
        source_count: card.source_count,
        confidence: card.confidence,
        project: card.project,
        topic: card
            .topic_categories
            .first()
            .cloned()
            .unwrap_or_default(),
        activity_type: card.activity_type,
        files_touched: card.files_touched,
        raw_snippets: card.raw_snippets,
        evidence_ids: card.evidence_ids,
    }
}
