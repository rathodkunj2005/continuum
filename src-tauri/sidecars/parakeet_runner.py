#!/usr/bin/env python3
"""Continuum local transcription runner.

Transcription strategy (in priority order):
1. NVIDIA NeMo Parakeet TDT 0.6B v3 (nvidia/parakeet-tdt-0.6b-v3) — best quality
2. faster-whisper small (CPU int8) — reliable fallback, no NVIDIA GPU needed
3. openai-whisper small — last resort
"""

import sys


def transcribe_with_parakeet(audio_path: str) -> str | None:
    """Use NVIDIA NeMo Parakeet TDT v3 for transcription."""
    try:
        import nemo.collections.asr as nemo_asr  # type: ignore

        model = nemo_asr.models.ASRModel.from_pretrained(
            model_name="nvidia/parakeet-tdt-0.6b-v3"
        )
        output = model.transcribe([audio_path])

        # The return value is a list of Hypothesis objects (NeMo >= 1.21)
        # or plain strings in older NeMo versions.
        if output:
            result = output[0]
            text = result.text if hasattr(result, "text") else str(result)
            text = text.strip()
            if text:
                return text
    except Exception:
        pass
    return None


def transcribe_with_faster_whisper(audio_path: str) -> str | None:
    """Use faster-whisper (CPU int8) as secondary backend."""
    try:
        from faster_whisper import WhisperModel  # type: ignore

        model = WhisperModel("small", device="cpu", compute_type="int8")
        segments, _ = model.transcribe(audio_path, beam_size=5, vad_filter=True)
        text = " ".join(
            s.text.strip() for s in segments if getattr(s, "text", "").strip()
        )
        if text.strip():
            return text.strip()
    except Exception:
        pass
    return None


def transcribe_with_openai_whisper(audio_path: str) -> str | None:
    """Use openai-whisper as last-resort fallback."""
    try:
        import whisper  # type: ignore

        model = whisper.load_model("small")
        result = model.transcribe(audio_path, fp16=False)
        text = (result.get("text", "") or "").strip()
        if text:
            return text
    except Exception as exc:
        print(f"[parakeet-runner unavailable: {exc}]", end="")
    return None


def main() -> int:
    if len(sys.argv) < 2:
        print("", end="")
        return 1

    audio_path = sys.argv[1]

    # 1. Primary: NVIDIA NeMo Parakeet TDT 0.6B v3
    text = transcribe_with_parakeet(audio_path)
    if text:
        print(text, end="")
        return 0

    # 2. Secondary: faster-whisper small
    text = transcribe_with_faster_whisper(audio_path)
    if text:
        print(text, end="")
        return 0

    # 3. Last resort: openai-whisper
    text = transcribe_with_openai_whisper(audio_path)
    if text:
        print(text, end="")
        return 0

    print("[parakeet-runner produced empty output]", end="")
    return 3


if __name__ == "__main__":
    raise SystemExit(main())
