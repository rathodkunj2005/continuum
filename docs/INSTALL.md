# Installing Continuum (for teammates)

This is the short path to a working Continuum on your Mac. The build ships with
the team's cloud config baked in and the text/image search models already
inside the app, so most features work immediately — you only download the local
language model once during onboarding.

> Requirements: Apple Silicon Mac (M1 or newer), macOS 13.0+.

## 1. Download

Grab the latest `*.dmg` from the repo's **Releases** page and open it, then drag
**Continuum** into your **Applications** folder.

## 2. Clear the quarantine flag (one time)

The build is ad-hoc signed but **not notarized** (we don't use a paid Apple
Developer account), so macOS quarantines it after download. Remove the flag:

```bash
xattr -dr com.apple.quarantine /Applications/continuum.app
```

Then launch Continuum from Applications.

> Alternative without Terminal: right-click the app → **Open**, and if macOS
> still blocks it, go to **System Settings → Privacy & Security**, scroll to the
> bottom, and click **Open Anyway**. On recent macOS the `xattr` command above is
> the most reliable.

Continuum runs as a **menu-bar app** (no Dock icon) — look for it in the menu bar
after launch. The onboarding window opens automatically on first run.

## 3. Grant macOS permissions

Onboarding will ask for these. Each opens the right System Settings pane:

- **Screen Recording** — required, this is how Continuum captures context.
- **Accessibility** — for focused-field detection and auto-fill.
- **Microphone** — only if you want voice search / meeting transcription.

After toggling a permission you may need to quit and reopen Continuum.

## 4. Download the local model

On the model step, pick the recommended **Qwen3-VL 2B** and let it download
(~2 GB total: the main model plus a vision projector). When it finishes,
onboarding completes and indexing starts.

That's it. Search, semantic recall, visual similarity, and cloud team sync work
from the start; the local model powers richer memory synthesis and Q&A.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| "continuum is damaged and can't be opened" | You skipped step 2. Run the `xattr -dr com.apple.quarantine …` command. |
| Stuck on the Screen Recording step | Grant it in System Settings → Privacy & Security → Screen Recording, then quit and reopen Continuum. |
| Cloud sign-in says "not configured" | The release wasn't built with the team secrets. Tell the maintainer (see `docs/RELEASE.md`). |
| Search returns nothing / "embedder" warnings in logs | The bundled models didn't seed. Reinstall, or run the bootstrap scripts in `README.md` into `~/Library/Application Support/com.continuum.app/models`. |
| Vision/"reads screenshots" features missing | The vision projector download failed during onboarding; re-run the model download from the in-app model panel. |

App data (models, database, config) lives under
`~/Library/Application Support/com.continuum.app/`.
