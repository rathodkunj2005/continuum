# 005: Capture Rate And Performance

Continuum captures enough screen context to make memory search useful without turning the desktop app into a constant high-load observer. The capture loop balances three pressures: recall, battery/CPU cost, and privacy. More frequent capture improves recall, but also increases OCR, embedding, storage, and model-summary work.

The stable loop uses a configurable base FPS, lower idle FPS, deep-idle pause, forced capture interval, perceptual deduplication, semantic deduplication, and batched LanceDB writes. This means a changing screen is captured, but repeated frames, unchanged OCR text, and idle periods are suppressed. The loop also batches records and flushes on a named interval or batch size rather than writing every frame individually.

OCR and embedding are the hot path. Apple Vision OCR runs only after privacy exclusions and image deduplication pass. Text embeddings are generated from compact OCR-aware chunks, with a small memo cache for repeated text inputs from the same capture burst. The capture loop records embedding availability and writes zero vectors when the semantic backend is unavailable, so the app can continue keyword search instead of blocking entirely.

The capture knobs live under `CapturePipelineConfig` alongside the existing FPS and dedupe settings. This keeps performance behavior visible and makes future tuning a configuration decision rather than another round of scattered magic numbers.
