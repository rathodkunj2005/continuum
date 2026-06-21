# Continuum Refactoring Skills

This repo now has four reusable Codex skills for behavior-preserving code quality work:

- `continuum-codebase-refactor`: architecture mapping, duplication discovery, maintainability risk review, and scoped refactoring.
- `continuum-clean-architecture`: concern separation, modular folder/module structure, and coupling reduction.
- `continuum-performance-optimization`: polling/render/async/storage/search optimization without behavior changes.
- `continuum-production-debugging`: root-cause analysis, edge-case review, and production-ready fixes.

## Application Order

1. Understand the current architecture and bulk.
2. Extract shared boundaries and utilities.
3. Optimize repeated work and cleanup paths.
4. Verify edge cases and run focused checks.

## Continuum Architecture Summary

Continuum is a local-first Tauri app. The React frontend in `src` owns the product shell, panels, onboarding, search UI, and Tauri command bindings. The Rust backend in `src-tauri/src` owns capture, privacy filtering, embeddings, search, storage, speech, meeting capture, MCP/Hermes integration, and command handlers.

Important data flow:

- Capture modules produce memory records.
- Store modules persist memory records and derived data.
- Embedding/search modules rank and synthesize results.
- Tauri API commands expose backend capabilities.
- React hooks/components poll or subscribe to status and render task-specific panels.

## Current Problem Areas

- Several React panels mix polling, async state transitions, formatting, and rendering in the same file.
- `src/app/App.tsx`, `src/domains/workspace/ControlPanel.tsx`, and `src/domains/workspace/AgentPanel.tsx` carry multiple responsibilities.
- Polling effects and timer cleanup are duplicated across components.
- Byte formatting and ID generation are duplicated.
- `src-tauri/src/ipc/commands/` is the main Tauri invoke surface; keep handlers thin and split further by domain when complexity grows.

## Refactoring Strategy

- Start with shared frontend helpers because they reduce duplication with low behavioral risk.
- Move feature-specific state machines into hooks when the render component becomes hard to scan.
- Split Rust command domains only after adding or identifying command-level tests.
- Keep public command names and serialized shapes stable.

## Applied Pass Results

### 1. Codebase Understanding And Refactoring

The first bulk source was repeated frontend infrastructure inside panels: interval setup/cleanup, async mounted guards, byte formatting, and client-side ID generation. These were extracted into shared helpers so panel files carry less repeated scaffolding.

### 2. Clean Architecture Rebuild

New shared boundaries:

- `src/shared/hooks/usePolling.ts`: reusable React polling with async mounted guards.
- `src/shared/utils/format.ts`: pure display formatting helpers.
- `src/shared/utils/id.ts`: pure client ID creation.

These keep feature components focused on feature state and rendering.

### 3. Performance Optimization Tips

Polling callbacks now use stable `useCallback` functions so unrelated renders do not restart intervals. Async polling paths check mount state before setting component state, preserving the old cleanup behavior while reducing duplicated effect code.

### 4. Senior Debugging Engineer

The extracted polling hook was reviewed for the production edge case where an async request resolves after unmount. The hook now passes an `isMounted` guard into callbacks, and converted call sites use it before mutating state. The ControlPanel test mock was also updated to cover the settings data that the component loads.

## Remaining High-Value Refactors

- Split `src/domains/workspace/ControlPanel.tsx` by settings, model, and storage sections.
- Split `src/domains/workspace/AgentPanel.tsx` into Hermes setup/chat subcomponents.
- Move command groups in `src-tauri/src/ipc/commands/` behind clearer domain modules with command-level regression tests.
