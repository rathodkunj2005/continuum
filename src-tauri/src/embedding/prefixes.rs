use crate::inference::model_config::{
    embedding_v5_contract, EmbeddingContractVersion, TextEmbeddingContract,
};

// The BGE v1.5 model card gives "Represent this sentence for searching
// relevant passages: " for queries and no prefix for documents. These two
// differ from it and are kept as they are (VS-88, decided 2026-10-08): no
// profile holds BGE rows, the model is not installed, and ADR 019 has not
// chosen the chunk model. Whoever changes them must clear the v5 memory and
// chunk tables first, because the reindex skips memories that already have
// rows and old and new vectors must never be searched together. The test
// `bge_document_and_query_prefixes_are_distinct_and_stable` fails on a change
// to make that step hard to miss.
pub const BGE_DOCUMENT_PREFIX: &str = "Represent this sentence: ";
pub const BGE_QUERY_PREFIX: &str = "Represent this question for searching relevant passages: ";
/// EmbeddingGemma's retrieval prompts, from its model card (VS-47).
pub const EMBEDDING_GEMMA_QUERY_PREFIX: &str = "task: search result | query: ";
pub const EMBEDDING_GEMMA_DOCUMENT_PREFIX: &str = "title: none | text: ";

/// A search query as `contract`'s model expects it.
pub fn query_text_for(contract: TextEmbeddingContract, text: &str) -> String {
    match contract.version {
        EmbeddingContractVersion::V4MiniLm384 => text.trim().to_string(),
        EmbeddingContractVersion::V5Bge1024 => prefix_once(text, BGE_QUERY_PREFIX),
        EmbeddingContractVersion::V6EmbeddingGemma
        | EmbeddingContractVersion::V7EmbeddingGemma2 => {
            prefix_once(text, EMBEDDING_GEMMA_QUERY_PREFIX)
        }
    }
}

/// A stored document as `contract`'s model expects it.
pub fn document_text_for(contract: TextEmbeddingContract, text: &str) -> String {
    match contract.version {
        EmbeddingContractVersion::V4MiniLm384 => text.trim().to_string(),
        EmbeddingContractVersion::V5Bge1024 => prefix_once(text, BGE_DOCUMENT_PREFIX),
        EmbeddingContractVersion::V6EmbeddingGemma
        | EmbeddingContractVersion::V7EmbeddingGemma2 => {
            prefix_once(text, EMBEDDING_GEMMA_DOCUMENT_PREFIX)
        }
    }
}

/// A document for the BGE chunk index. One source with `document_text_for`.
pub fn prefix_document_for_index(text: &str) -> String {
    document_text_for(embedding_v5_contract(), text)
}

/// A query for the BGE chunk route. One source with `query_text_for`.
pub fn prefix_query_for_search(text: &str) -> String {
    query_text_for(embedding_v5_contract(), text)
}

fn prefix_once(text: &str, prefix: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with(prefix) {
        trimmed.to_string()
    } else {
        format!("{prefix}{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bge_document_and_query_prefixes_are_distinct_and_stable() {
        let text = "memory vault graph work";

        assert_eq!(
            prefix_document_for_index(text),
            "Represent this sentence: memory vault graph work"
        );
        assert_eq!(
            prefix_query_for_search(text),
            "Represent this question for searching relevant passages: memory vault graph work"
        );
        assert_ne!(
            prefix_document_for_index(text),
            prefix_query_for_search(text)
        );
    }

    #[test]
    fn each_contract_gets_its_own_prompts() {
        use crate::inference::model_config::{
            embedding_v4_contract, embedding_v5_contract, embedding_v6_contract,
        };
        let gemma = embedding_v6_contract(768).expect("supported dimension");
        assert_eq!(
            query_text_for(gemma, " where is the runbook "),
            "task: search result | query: where is the runbook"
        );
        assert_eq!(
            document_text_for(gemma, "Postgres 16 runbook"),
            "title: none | text: Postgres 16 runbook"
        );
        assert_eq!(
            query_text_for(embedding_v5_contract(), "x"),
            prefix_query_for_search("x")
        );
        assert_eq!(query_text_for(embedding_v4_contract(), " x "), "x");
        // Prompting twice does not stack the prompt.
        let once = query_text_for(gemma, "x");
        assert_eq!(query_text_for(gemma, &once), once);
    }

    #[test]
    fn bge_prefix_helpers_do_not_double_prefix() {
        let prefixed = "Represent this sentence: memory vault graph work";
        assert_eq!(prefix_document_for_index(prefixed), prefixed);

        let query_prefixed =
            "Represent this question for searching relevant passages: memory vault graph work";
        assert_eq!(prefix_query_for_search(query_prefixed), query_prefixed);
    }
}
