# 001: Why LanceDB

Continuum stores local memories as compact records with several retrieval shapes: recent browsing by time, keyword search by text, and semantic search by embedding. LanceDB is the right fit for the current stable pipeline because it keeps vector storage local, has a simple table model, and lets the Rust backend query vector columns without running a separate database service.

The app needs fast-enough approximate nearest-neighbor retrieval for a desktop memory store, not a distributed search cluster. LanceDB keeps setup small for a macOS app: the user grants permissions, downloads local models, and the database lives under app data. That matches the local-first privacy promise better than a hosted vector database.

Continuum also stores non-vector metadata beside embeddings: app name, bundle id, window title, URL, OCR confidence, noise score, day bucket, snippet, session key, lexical shadow, decay score, and last accessed time. LanceDB lets these values travel with the vector row, which keeps the search pipeline straightforward. Hybrid search can retrieve semantic candidates, keyword candidates, and metadata-filtered candidates, then rerank them in application code.

The tradeoff is migration care. Embedding dimension changes affect fixed-size vector columns, and old prototype schemas can become unreadable without an explicit reset or migration. For that reason the store validates vector dimensions at startup, repairs malformed incoming vectors before indexing, and keeps destructive schema changes out of normal cleanup work.
