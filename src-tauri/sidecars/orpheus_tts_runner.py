#!/usr/bin/env python3
"""Continuum Orpheus GGUF sidecar.

Usage:
    python3 orpheus_tts_runner.py <model_path> <output_path> <voice_id> <text>
"""

from __future__ import annotations

import os
import platform
import sys
import wave
from typing import Iterable

import numpy as np

CUSTOM_TOKEN_PREFIX = "<custom_token_"
VALID_VOICES = {"tara", "leah", "jess", "leo", "dan", "mia", "zac", "zoe"}


def choose_gpu_layers() -> int:
    if sys.platform == "darwin" and platform.machine() == "arm64":
        return -1
    return 0


def load_snac_session():
    from huggingface_hub import hf_hub_download  # type: ignore
    import onnxruntime  # type: ignore

    decoder_path = hf_hub_download(
        repo_id="onnx-community/snac_24khz-ONNX",
        subfolder="onnx",
        filename="decoder_model.onnx",
    )
    providers = [
        provider
        for provider in ("CoreMLExecutionProvider", "CPUExecutionProvider")
        if provider in onnxruntime.get_available_providers()
    ]
    if not providers:
        providers = ["CPUExecutionProvider"]
    return onnxruntime.InferenceSession(decoder_path, providers=providers)


def token_to_audio_id(token_text: str, index: int) -> int | None:
    token_text = token_text.strip()
    token_start = token_text.rfind(CUSTOM_TOKEN_PREFIX)
    if token_start == -1:
        return None

    token = token_text[token_start:]
    if not token.startswith(CUSTOM_TOKEN_PREFIX) or not token.endswith(">"):
        return None

    try:
        raw_value = int(token[len(CUSTOM_TOKEN_PREFIX) : -1])
    except ValueError:
        return None

    return raw_value - 10 - ((index % 7) * 4096)


def convert_to_audio_bytes(snac_session, multiframe: list[int]) -> bytes | None:
    if len(multiframe) < 28:
        return None

    frame_count = len(multiframe) // 7
    frame = multiframe[: frame_count * 7]

    codes_0 = np.array([], dtype=np.int32)
    codes_1 = np.array([], dtype=np.int32)
    codes_2 = np.array([], dtype=np.int32)

    for frame_index in range(frame_count):
        offset = frame_index * 7
        codes_0 = np.append(codes_0, frame[offset])
        codes_1 = np.append(codes_1, [frame[offset + 1], frame[offset + 4]])
        codes_2 = np.append(
            codes_2,
            [frame[offset + 2], frame[offset + 3], frame[offset + 5], frame[offset + 6]],
        )

    if (
        np.any(codes_0 < 0)
        or np.any(codes_0 > 4096)
        or np.any(codes_1 < 0)
        or np.any(codes_1 > 4096)
        or np.any(codes_2 < 0)
        or np.any(codes_2 > 4096)
    ):
        return None

    inputs = {
        name: value
        for name, value in zip(
            [item.name for item in snac_session.get_inputs()],
            [
                np.expand_dims(codes_0, axis=0),
                np.expand_dims(codes_1, axis=0),
                np.expand_dims(codes_2, axis=0),
            ],
        )
    }
    audio_hat = snac_session.run(None, inputs)[0]
    audio_window = audio_hat[:, :, 2048:4096]
    return (audio_window * 32767).astype(np.int16).tobytes()


def stream_audio_bytes(llm, snac_session, prompt: str) -> Iterable[bytes]:
    token_buffer: list[int] = []
    emitted = 0
    stream = llm(
        prompt,
        max_tokens=2048,
        stream=True,
        temperature=0.8,
        top_p=0.95,
        top_k=40,
        repeat_penalty=1.1,
    )

    for chunk in stream:
        token_text = chunk["choices"][0].get("text", "")
        token_id = token_to_audio_id(token_text, emitted)
        if token_id is None or token_id <= 0:
            continue

        token_buffer.append(token_id)
        emitted += 1

        if emitted % 7 == 0 and emitted > 27:
            audio_bytes = convert_to_audio_bytes(snac_session, token_buffer[-28:])
            if audio_bytes:
                yield audio_bytes


def main() -> int:
    if len(sys.argv) < 5:
        print(
            "usage: orpheus_tts_runner.py <model_path> <output_path> <voice_id> <text>",
            file=sys.stderr,
        )
        return 1

    model_path = sys.argv[1]
    output_path = sys.argv[2]
    voice_id = sys.argv[3].strip().lower() or "tara"
    text = sys.argv[4].strip()

    if voice_id not in VALID_VOICES:
        print(
            f"Unsupported Orpheus voice '{voice_id}'. Choose one of: {', '.join(sorted(VALID_VOICES))}",
            file=sys.stderr,
        )
        return 1
    if not os.path.isfile(model_path):
        print(f"Orpheus model not found: {model_path}", file=sys.stderr)
        return 1
    if not text:
        print("Cannot synthesize empty text", file=sys.stderr)
        return 1

    os.makedirs(os.path.dirname(output_path), exist_ok=True)

    try:
        from llama_cpp import Llama  # type: ignore
    except Exception as exc:
        print(f"Failed importing llama_cpp: {exc}", file=sys.stderr)
        return 2

    try:
        llm = Llama(
            model_path=model_path,
            n_ctx=4096,
            verbose=False,
            n_gpu_layers=choose_gpu_layers(),
            n_threads=max(1, os.cpu_count() or 4),
        )
        snac_session = load_snac_session()
    except Exception as exc:
        print(f"Failed initializing Orpheus runtime: {exc}", file=sys.stderr)
        return 3

    prompt = f"<|audio|>{voice_id}: {text}<|eot_id|><custom_token_4>"
    audio_buffer = bytearray()

    try:
        for audio_chunk in stream_audio_bytes(llm, snac_session, prompt):
            audio_buffer.extend(audio_chunk)
    except Exception as exc:
        print(f"Orpheus synthesis failed: {exc}", file=sys.stderr)
        return 4

    if not audio_buffer:
        print("Orpheus produced no audio frames", file=sys.stderr)
        return 5

    with wave.open(output_path, "wb") as wav_file:
        wav_file.setnchannels(1)
        wav_file.setsampwidth(2)
        wav_file.setframerate(24000)
        wav_file.writeframes(bytes(audio_buffer))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
