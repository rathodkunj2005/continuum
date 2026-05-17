use crate::context_runtime::query_plan::PlannerIntent;
use crate::graph::schema::{GraphEdgeType, GraphNodeType};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct GraphPlan {
    pub max_hops: u8,
    pub seed_kinds: Vec<GraphNodeType>,
    pub allowed_edges: Vec<GraphEdgeType>,
}

impl GraphPlan {
    pub fn from_intent(intent: PlannerIntent) -> Self {
        match intent {
            PlannerIntent::Definition => Self {
                max_hops: 2,
                seed_kinds: vec![GraphNodeType::Concept, GraphNodeType::Memory],
                allowed_edges: vec![
                    GraphEdgeType::BelongsToProject,
                    GraphEdgeType::MentionedIn,
                    GraphEdgeType::EvidencedBy,
                ],
            },
            PlannerIntent::Debug => Self {
                max_hops: 2,
                seed_kinds: vec![
                    GraphNodeType::Error,
                    GraphNodeType::File,
                    GraphNodeType::Command,
                    GraphNodeType::Memory,
                ],
                allowed_edges: vec![
                    GraphEdgeType::Causes,
                    GraphEdgeType::Resolves,
                    GraphEdgeType::FixedBy,
                    GraphEdgeType::BrokeBy,
                    GraphEdgeType::TriggeredBy,
                ],
            },
            PlannerIntent::ResumeWork => Self {
                max_hops: 2,
                seed_kinds: vec![
                    GraphNodeType::Project,
                    GraphNodeType::Session,
                    GraphNodeType::Memory,
                    GraphNodeType::Task,
                ],
                allowed_edges: vec![
                    GraphEdgeType::OccurredInSession,
                    GraphEdgeType::BelongsToProject,
                    GraphEdgeType::PrecededBy,
                    GraphEdgeType::FollowedBy,
                    GraphEdgeType::SameTaskAs,
                ],
            },
            PlannerIntent::Lookup => Self {
                max_hops: 1,
                seed_kinds: vec![
                    GraphNodeType::Memory,
                    GraphNodeType::Concept,
                    GraphNodeType::File,
                    GraphNodeType::Url,
                ],
                allowed_edges: vec![GraphEdgeType::MentionedIn, GraphEdgeType::BelongsToProject],
            },
            PlannerIntent::HowTo => Self {
                max_hops: 1,
                seed_kinds: vec![
                    GraphNodeType::Tool,
                    GraphNodeType::Command,
                    GraphNodeType::File,
                    GraphNodeType::Memory,
                ],
                allowed_edges: vec![
                    GraphEdgeType::UsedIn,
                    GraphEdgeType::MentionedIn,
                    GraphEdgeType::EvidencedBy,
                ],
            },
            PlannerIntent::Timeline => Self {
                max_hops: 0,
                seed_kinds: vec![GraphNodeType::Session, GraphNodeType::Memory],
                allowed_edges: vec![GraphEdgeType::PrecededBy, GraphEdgeType::FollowedBy],
            },
            PlannerIntent::RelatedTo => Self {
                max_hops: 2,
                seed_kinds: vec![GraphNodeType::Concept, GraphNodeType::Memory],
                allowed_edges: vec![GraphEdgeType::SimilarTo, GraphEdgeType::MentionedIn],
            },
        }
    }
}

impl From<PlannerIntent> for GraphPlan {
    fn from(intent: PlannerIntent) -> Self {
        Self::from_intent(intent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_plan_whitelists_debug_edges() {
        let plan = GraphPlan::from_intent(PlannerIntent::Debug);

        assert_eq!(plan.max_hops, 2);
        assert!(plan.allowed_edges.contains(&GraphEdgeType::Causes));
        assert!(plan.allowed_edges.contains(&GraphEdgeType::TriggeredBy));
    }

    #[test]
    fn timeline_plan_does_not_expand_neighbors() {
        let plan = GraphPlan::from_intent(PlannerIntent::Timeline);

        assert_eq!(plan.max_hops, 0);
    }
}
