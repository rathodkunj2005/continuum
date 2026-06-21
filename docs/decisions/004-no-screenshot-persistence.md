# 004: No Screenshot Persistence

Continuum's stable memory pipeline should not persist raw screenshots. The capture loop needs pixel data temporarily so Apple Vision can run OCR and so frame deduplication can avoid repeated work. After that, the durable memory record should contain compact text, metadata, embeddings, and summaries rather than raw screen pixels.

This decision keeps the product aligned with the local-first privacy promise. Screen pixels can contain passwords, private messages, banking data, health information, or content from apps the user did not intend to search later. Even when the database is local, retaining screenshots creates more sensitive data than the current stable search experience needs.

Privacy exclusions are checked before screen capture and OCR. Blocklisted apps, internal Continuum windows, and blocked URLs or titles are skipped before the expensive and sensitive parts of the pipeline run. Sensitive-context alerts are separate from the blocklist: they can warn the user about potentially private screens, but they do not justify persisting pixels.

The LanceDB schema still contains screenshot/image-related fields for compatibility with older records and adjacent experimental work. Capture records set `screenshot_path` to `None`. Store compaction also clears screenshot paths before indexing compact memory payloads. Visual semantic search can be reintroduced later only with an explicit privacy design.

## Update 2026-05-13: CLIP image vectors on screen captures

Screen captures now compute and store a 512-d CLIP image embedding alongside the existing text embeddings. The vector is derived from the same pixel buffer that already passes through Apple Vision OCR; **no raw pixels are persisted** (`screenshot_path` remains `None`). The vector is a compact, L2-normalized float32 representation — not a screenshot.

The CLIP embedding step runs **after** every existing privacy and signal gate fires (blocklist, internal-app exclusion, sensitive-context detection, surface policy, OCR low-signal, noise score, semantic dedup, grounding floor). Frames that are dropped before storage are never embedded. The embedding inherits the same protection as the OCR text it accompanies.

A new image-to-image retrieval surface (`find_visually_similar_memories`) uses cosine similarity over the `image_embedding` column. Cross-modal text->image retrieval (e.g. searching captures with a free-text query routed through a CLIP text tower) remains explicitly out of scope until a separate privacy design is documented. Records that pre-date this change keep their zero image vector and are filtered out of image-to-image results.
