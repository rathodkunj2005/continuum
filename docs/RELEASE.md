# Releasing Continuum (for maintainers)

How to cut a packaged `.dmg` that teammates can install and use with all
core features working from first launch. The teammate-facing steps live in
[`INSTALL.md`](INSTALL.md).

## What "works from the get-go" means here

| Capability | How it ships | Where |
| --- | --- | --- |
| Text + image search models (MiniLM, CLIP) | Bundled in the `.app`, seeded to app-data on first run | `scripts/release/stage-bundled-models.sh`, `seed_bundled_models` in `src-tauri/src/main.rs` |
| Cloud team sync (Supabase + agent-sync) | Secrets baked into the binary at build time | `BAKED_SECRETS` in `src-tauri/src/main.rs`, CI env |
| Local LLM/VLM (Qwen3-VL 2B + mmproj) | Downloaded in-app during onboarding | `do_download` in `src-tauri/src/ipc/onboarding.rs` |
| Gatekeeper bypass | Ad-hoc signed; teammate clears quarantine once | `tauri.conf.json` `signingIdentity: "-"`, `INSTALL.md` |

## One-time setup: GitHub Actions secrets

Set these under **Settings → Secrets and variables → Actions → New repository
secret**. The release workflow maps them to the compile-time `CONTINUUM_BAKED_*`
variables; missing/empty ones just make that feature report "not configured".

| GitHub secret | Required? | Purpose |
| --- | --- | --- |
| `CONTINUUM_SUPABASE_URL` | Yes (for cloud) | Supabase project URL |
| `CONTINUUM_SUPABASE_ANON_KEY` | Yes (for cloud) | Public anon key (safe to ship; RLS-scoped) |
| `CONTINUUM_SUPABASE_FUNCTIONS_URL` | Optional | Defaults to `${SUPABASE_URL}/functions/v1` |
| `CONTINUUM_AGENT_SYNC_SECRET` | Yes (to push to team graph) | Shared secret for the `agent-sync` Edge Function |
| `CONTINUUM_ANTHROPIC_API_KEY` | Optional | Only used by the experimental Python agent runner |

> **Security:** baked secrets are extractable from the distributed binary (e.g.
> `strings continuum`). Only the Supabase **anon** key is designed to be public.
> The `AGENT_SYNC_SECRET` and `ANTHROPIC_API_KEY` should be treated as
> **burnable** — use a dedicated key for distributed builds and rotate it if the
> `.dmg` leaks outside the team. Secrets live only in GitHub Actions and are
> never committed to the repo.

## Cut a release

1. Bump the version in `package.json`, `src-tauri/tauri.conf.json`,
   `src-tauri/Cargo.toml`, and `src-tauri/Info.plist` (keep them in sync).
2. Commit and push to `main`.
3. Tag and push:

   ```bash
   git tag v0.2.12
   git push origin v0.2.12
   ```

4. The **Release (macOS .dmg)** workflow (`.github/workflows/release.yml`)
   builds on an Apple Silicon runner, stages the bundled models, bakes in the
   secrets, builds the `.dmg`, and attaches it to the GitHub Release for the tag.
5. Share the Release link. Teammates follow [`INSTALL.md`](INSTALL.md).

You can also trigger it manually from the Actions tab (**Run workflow**) and
pass a `tag` input.

## Build it locally (optional)

```bash
# stage the bundled models into the Tauri resource source dir
bash scripts/release/stage-bundled-models.sh src-tauri/bundled-models

# bake secrets just for this build (leave unset to omit a feature)
export CONTINUUM_BAKED_SUPABASE_URL=...      \
       CONTINUUM_BAKED_SUPABASE_ANON_KEY=... \
       CONTINUUM_BAKED_AGENT_SYNC_SECRET=...

npm ci
npm run tauri build -- --target aarch64-apple-darwin
# .dmg lands in src-tauri/target/release/bundle/dmg/
```

Local dev (`npm run tauri dev`) is unaffected: the bundled-models resource dir
is empty so nothing is seeded, and unset `CONTINUUM_BAKED_*` means the app keeps
reading real values from `.env` exactly as before.

## Notes & limitations

- **Architecture:** the workflow builds **arm64 only** (Apple Silicon). Add an
  Intel/universal job if a teammate is on an Intel Mac.
- **No notarization:** without a paid Apple Developer ID we can't notarize, so
  the one-time quarantine step in `INSTALL.md` is expected. If the team later
  gets a Developer ID, set `signingIdentity` to it plus `APPLE_ID` /
  `APPLE_PASSWORD` / `APPLE_TEAM_ID` env in the workflow to enable notarization.
- **Download size:** the `.dmg` carries ~150 MB of embedding models; the ~2 GB
  Qwen3-VL download happens in-app so it isn't re-downloaded on every release.
