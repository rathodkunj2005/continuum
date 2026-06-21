# 003: Hybrid Search Ranking

Continuum uses hybrid search because neither vector search nor keyword search is reliable enough alone for screen memory. OCR text can be noisy, short, duplicated, or missing the exact words a user asks for. At the same time, semantic embeddings can blur precise identifiers such as numbers, filenames, URLs, and product names.

The stable ranking path runs semantic retrieval over the main text embedding, optional snippet-vector retrieval when the query has enough terms, and keyword retrieval over lexical text. The candidates are fused by memory id, scored with configurable vector, snippet, and keyword weights, then reranked with query intent, term coverage, source alignment, OCR confidence, noise score, recency, decay score, and light diversity.

The goal is not to build a universal ranking engine. It is to make the common memory query work: "what was that page I saw?", "where did I read about this error?", "show the launch checklist", or "what was the last spreadsheet about budget?". Semantic search helps with paraphrase and fuzzy recall. Keyword search preserves exact-match behavior. Reranking keeps obviously stale, noisy, or weakly related captures from crowding the card UI.

Ranking knobs live in `SearchConfig` so weights, limits, branch timeouts, relevance floors, and diversity behavior have names. Tests cover vector/keyword fusion and score-order preservation so future ranking changes have guardrails.
