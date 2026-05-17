# FNDR Setup Guide

## Prerequisites
- macOS 13.0+
- Xcode Command Line Tools
- Node.js (v18+)
- Rust (for Tauri)
- Python 3.9+

## Initial Setup

### 1. Install Dependencies
```bash
npm install
```

### 2. Download Embedding Model (required for search)
Memory search requires the All-MiniLM-L6-v2 embedding model:

```bash
./scripts/bootstrap/download-minilm.sh
```

Models download to: `~/Library/Application Support/com.fndr.FNDR/models/`

**Note:** One-time setup. Models persist across rebuilds and cache clears.

### 3. Start Development Server
```bash
npm run tauri dev
```

The app opens at `http://127.0.0.1:1420`

## Local Models

FNDR uses two local models, both running on your Mac with no external API calls:

### 1. All-MiniLM-L6-v2 (Embedding/Search)
- **Purpose:** Memory search with semantic understanding
- **Size:** ~90 MB on disk
- **RAM:** ~0.5 GB
- **Dimensions:** 384-dimensional text embeddings
- **Download:** `./scripts/bootstrap/download-minilm.sh` (required)

### 2. Qwen3-VL-2B (Multimodal/Memory Creation)
- **Purpose:** Screen capture, OCR, and structured memory extraction
- **Size:** ~1.5 GB on disk
- **RAM:** ~3.5 GB
- **Type:** Vision Language Model (multimodal)
- **Download:** Automatic on first memory capture

## Troubleshooting

### Search embeddings show "unavailable"
The MiniLM embedding model wasn't downloaded. Run:
```bash
./scripts/bootstrap/download-minilm.sh
```

### Port 1420 already in use
Kill the previous dev process:
```bash
lsof -i :1420 | awk '{print $2}' | xargs kill -9
npm run tauri dev
```

### Models missing after full system cache clear
Models are stored in `~/Library/Application Support/com.fndr.FNDR/models/` (outside dev cache), so they normally persist. If missing, re-run the download script:
```bash
./scripts/bootstrap/download-minilm.sh
```
