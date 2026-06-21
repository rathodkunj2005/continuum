#!/usr/bin/env python3
"""Continuum Whisper GGUF sidecar.

Usage:
    python3 whisper_gguf_runner.py <model_path> <audio_path> [--voice-command]
"""

from __future__ import annotations

import inspect
import os
import shutil
import subprocess
import sys
import tempfile
from typing import Iterable

VOICE_COMMAND_HINTS = (
    "search",
    "find",
    "look for",
    "clear search",
    "pause capture",
    "resume capture",
    "open meetings",
    "close meetings",
    "open graph",
    "close graph",
)

VOICE_INITIAL_PROMPT = (
    "Voice command examples: search for canva, find cricket highlights, clear search, "
    "pause capture, resume capture, open meetings, close meetings, open graph, close graph."
)


def _extract_text(result: object) -> str:
    """Best-effort text extraction across whisper_cpp_python result formats."""
    if result is None:
        return ""

    if isinstance(result, str):
        return " ".join(result.split()).strip()

    if isinstance(result, dict):
        direct = str(result.get("text") or "").strip()
        if direct:
            return " ".join(direct.split()).strip()

        segments = result.get("segments")
        if isinstance(segments, list):
            parts: list[str] = []
            for segment in segments:
                if isinstance(segment, dict):
                    seg_text = str(segment.get("text") or "").strip()
                    if seg_text:
                        parts.append(seg_text)
            joined = " ".join(parts).strip()
            if joined:
                return " ".join(joined.split()).strip()

    text_attr = getattr(result, "text", None)
    if text_attr:
        return " ".join(str(text_attr).split()).strip()

    return ""


def _result_confidence(result: object) -> float:
    """Estimate confidence from optional Whisper segment metadata."""
    if not isinstance(result, dict):
        return 0.45

    segments = result.get("segments")
    if not isinstance(segments, list) or not segments:
        return 0.45

    avg_logprobs: list[float] = []
    no_speech_probs: list[float] = []

    for segment in segments:
        if not isinstance(segment, dict):
            continue

        logp = segment.get("avg_logprob")
        if isinstance(logp, (int, float)):
            avg_logprobs.append(float(logp))

        no_speech = segment.get("no_speech_prob")
        if isinstance(no_speech, (int, float)):
            no_speech_probs.append(float(no_speech))

    signals: list[float] = []
    if avg_logprobs:
        # Typical range: [-5, 0], where closer to 0 is better.
        avg = sum(avg_logprobs) / len(avg_logprobs)
        signals.append(max(0.0, min(1.0, (avg + 5.0) / 5.0)))

    if no_speech_probs:
        avg_no_speech = sum(no_speech_probs) / len(no_speech_probs)
        signals.append(max(0.0, min(1.0, 1.0 - avg_no_speech)))

    if not signals:
        return 0.45

    return sum(signals) / len(signals)


def _quality_score(result: object, text: str, voice_command_mode: bool) -> float:
    words = text.split()
    if not words:
        return -1e9

    lowered = text.lower()
    unique_ratio = len(set(word.lower() for word in words)) / max(1, len(words))

    score = min(len(words), 18) * 0.65
    score += unique_ratio * 1.5
    score += _result_confidence(result) * 2.4

    if voice_command_mode:
        if 1 <= len(words) <= 14:
            score += 1.0
        if len(words) > 22:
            score -= 1.6
        if any(hint in lowered for hint in VOICE_COMMAND_HINTS):
            score += 1.25

    # Penalize obvious low-signal artifacts.
    if lowered in {"you", "okay", "thanks"}:
        score -= 0.8

    return score


def _supports_kwargs(callable_obj: object, kwargs: dict[str, object]) -> dict[str, object]:
    """Filter kwargs to those accepted by the transcribe callable."""
    try:
        signature = inspect.signature(callable_obj)
    except (TypeError, ValueError):
        return kwargs

    params = signature.parameters
    if any(param.kind == inspect.Parameter.VAR_KEYWORD for param in params.values()):
        return kwargs

    return {key: value for key, value in kwargs.items() if key in params}


def _transcribe_profiles(voice_command_mode: bool) -> list[dict[str, object]]:
    common = [
        {
            "language": "en",
            "temperature": 0.0,
            "beam_size": 5,
            "best_of": 5,
            "condition_on_previous_text": False,
        },
        {
            "language": "en",
            "temperature": 0.0,
            "best_of": 3,
        },
        {
            "language": "en",
            "temperature": 0.0,
        },
        {
            "language": "en",
        },
        {},
    ]

    if not voice_command_mode:
        return common

    with_prompt: list[dict[str, object]] = []
    for profile in common:
        candidate = dict(profile)
        candidate["initial_prompt"] = VOICE_INITIAL_PROMPT
        with_prompt.append(candidate)

    return with_prompt + common


def _run_ffmpeg_convert(src: str, dst: str, filters: str | None = None) -> bool:
    cmd = [
        "ffmpeg",
        "-nostdin",
        "-y",
        "-i",
        src,
    ]

    if filters:
        cmd.extend(["-af", filters])

    cmd.extend([
        "-ar",
        "16000",
        "-ac",
        "1",
        "-c:a",
        "pcm_s16le",
        dst,
    ])

    completed = subprocess.run(
        cmd,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return completed.returncode == 0 and os.path.isfile(dst)


def _build_audio_candidates(audio_path: str, voice_command_mode: bool) -> list[str]:
    temp_root = tempfile.mkdtemp(prefix="continuum-whisper-")

    raw_wav = os.path.join(temp_root, "input.wav")
    candidates: list[str] = []

    if _run_ffmpeg_convert(audio_path, raw_wav):
        candidates.append(raw_wav)
    else:
        # If conversion fails (e.g. missing ffmpeg codecs), allow Whisper to attempt raw input.
        candidates.append(audio_path)

    if voice_command_mode:
        enhanced_wav = os.path.join(temp_root, "enhanced.wav")
        source = candidates[0]
        # Light denoise-ish filter chain plus gain for short microphone commands.
        enhanced_filter = "highpass=f=90,lowpass=f=7600,volume=1.6"
        if _run_ffmpeg_convert(source, enhanced_wav, enhanced_filter):
            candidates.insert(0, enhanced_wav)

    # stash cleanup path on the function for finally block
    _build_audio_candidates._temp_root = temp_root  # type: ignore[attr-defined]
    return candidates


def _cleanup_candidates_temp_root() -> None:
    temp_root = getattr(_build_audio_candidates, "_temp_root", None)
    if temp_root and isinstance(temp_root, str):
        shutil.rmtree(temp_root, ignore_errors=True)


def _response_formats() -> Iterable[str]:
    return ("verbose_json", "json", "text")


def _best_transcript(whisper: object, audio_candidates: list[str], voice_command_mode: bool) -> str:
    profiles = _transcribe_profiles(voice_command_mode)

    best_text = ""
    best_score = -1e9

    for audio_candidate in audio_candidates:
        for profile in profiles:
            for response_format in _response_formats():
                kwargs = dict(profile)
                if response_format != "text":
                    kwargs["response_format"] = response_format

                kwargs = _supports_kwargs(getattr(whisper, "transcribe"), kwargs)

                try:
                    with open(audio_candidate, "rb") as audio_file:
                        result = whisper.transcribe(audio_file, **kwargs)
                except TypeError:
                    # Different whisper_cpp_python builds expose slightly different kwargs.
                    continue
                except Exception:
                    continue

                text = _extract_text(result)
                if not text:
                    continue

                score = _quality_score(result, text, voice_command_mode)
                if score > best_score:
                    best_score = score
                    best_text = text

                # Early exit if we found a strong voice command transcript.
                if voice_command_mode and score >= 6.2:
                    return best_text

    return best_text


def main() -> int:
    if len(sys.argv) < 3:
        print(
            "usage: whisper_gguf_runner.py <model_path> <audio_path> [--voice-command]",
            file=sys.stderr,
        )
        return 1

    model_path = sys.argv[1]
    audio_path = sys.argv[2]
    voice_command_mode = "--voice-command" in sys.argv[3:]

    if not os.path.isfile(model_path):
        print(f"Whisper model not found: {model_path}", file=sys.stderr)
        return 1
    if not os.path.isfile(audio_path):
        print(f"Audio input not found: {audio_path}", file=sys.stderr)
        return 1

    try:
        from whisper_cpp_python import Whisper  # type: ignore
    except Exception as exc:
        print(f"Failed importing whisper_cpp_python: {exc}", file=sys.stderr)
        return 2

    try:
        whisper = Whisper(model_path=model_path)
        candidates = _build_audio_candidates(audio_path, voice_command_mode)
        transcript = _best_transcript(whisper, candidates, voice_command_mode)

        if transcript:
            print(transcript, end="")
            return 0
    except Exception as exc:
        print(f"Whisper transcription failed: {exc}", file=sys.stderr)
        return 3
    finally:
        _cleanup_candidates_temp_root()

    print("Whisper returned an empty transcript", file=sys.stderr)
    return 4


if __name__ == "__main__":
    raise SystemExit(main())
