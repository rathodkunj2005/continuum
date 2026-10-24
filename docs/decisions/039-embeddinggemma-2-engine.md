# ADR 039: EmbeddingGemma 2 as the text embedding engine

## Status

Experiment on branch `kunj/embeddinggemma2-engine`. Not proposed for main until the thresholds below are recalibrated.

## Context

Text embeddings were all-MiniLM-L6-v2 (384-d, contract v4) through `ort`. DigUp embeds files with EmbeddingGemma 2 (Q8_0 GGUF) on a patched llama.cpp and measures cosine 0.999 or better against the mlx-vlm reference. This branch asks whether the same engine, behind FNDR's unchanged capture, chunking, cache, dedup and retrieval code, improves search.

## Decision

- New contract v7: `google/embeddinggemma-2`, `embeddinggemma-2-Q8_0.gguf`, 768-d, table `memories_v7_embeddinggemma2_768`, 2048-token inputs. It is the active contract. The v4 table is left in place.
- The model runs in `fndr-eg2-helper` (`src-tauri/helpers/fndr-eg2`), a child process with llama.cpp b11461 plus DigUp's no-logits patch (`scripts/eg2/build-llama.sh`). `llama-cpp-2` cannot load this model, and linking a second ggml into the app collides with it. JSON lines over stdin and stdout; the client restarts a dead helper once.
- Prompts are DigUp's: `task: search result | query: ` and `title: none | text: `. The legacy `embed_batch` entry points carry no role, so under v7 they apply the document prompt after chunking; text already carrying the query prompt is left alone.
- The mock fallback stays off. A missing helper or model is an error, not a silent MiniLM.
- Not changed: capture cadence, chunking, the embedding cache, admission gate, dedup, fusion weights, every score threshold.

## Evidence

`fndr-eg2-helper --check` on DigUp's reference texts: cosine 1.00000 on 5 of 5 inputs, 1.00000 batched. `make qa-retrieval-check` against the MiniLM references, each persona in its own scratch profile:

| Persona | Recall@5 ref to now | MRR@10 ref to now | Gate |
|---|---|---|---|
| knowledge-worker | 1.000 to 0.955 | 0.966 to 0.937 | pass |
| office-pm | 0.900 to 0.850 | 0.661 to 0.785 | fail (one paraphrase went from rank 9 to a miss) |
| software-engineer | 1.000 to 1.000 | 0.831 to 0.883 | pass |

Search p95 fell from 315 to 372 ms to 167 to 199 ms. The reference p95 figures come from another run, so the timing comparison is indicative only.

Negative queries break. The strong-match bar is 0.25; the median top score of a no-match query is now 0.55 to 0.60 (positives 0.71 to 0.77), so 0 of 8 negatives per persona fall under the bar and the product reports "strong match" for all of them. The bar, and the other embedding-score constants (focus drift 0.30, proactive 0.82, same task 0.80, legacy graph link 0.82), were set for MiniLM's score range.

## Consequences

- Reindex: vectors in the v4 table are not readable under v7. Existing profiles need `reembed_memories` or a fresh profile, and a `config.toml` that still says 384 fails validation at startup by design.
- Recalibrate the strong-match bar and the constants above on the retrieval sets before any merge.
- Packaging: the helper is not yet in `externalBin`; development builds find it through `FNDR_EG2_HELPER` or `src-tauri/helpers/fndr-eg2/target/release`.
- The BGE v5 chunk route is a separate engine and is untouched.
- Gemma Terms of Use apply (ADR 019).
