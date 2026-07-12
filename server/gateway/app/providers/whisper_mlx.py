"""Apple Silicon Whisper STT via MLX/Metal.

This provider is intentionally separate from faster-whisper: MLX executes the
model on Apple's GPU while the CPU continues handling capture, WebSockets, VAD,
and text insertion. It is selected automatically only on arm64 macOS.

Env:
  WHISPR_MLX_MODEL       MLX model repo/path override
  WHISPR_WHISPER_MODEL   friendly alias shared with the desktop settings
"""

import asyncio
import logging
import os
import threading

from .base import SttSession

logger = logging.getLogger("whispr.whisper_mlx")

SAMPLE_RATE = 16_000
PARTIAL_INTERVAL_S = 1.0
MIN_PARTIAL_AUDIO_S = 0.6

_INFERENCE_LOCK = threading.Lock()

_MODEL_ALIASES = {
    "tiny": "mlx-community/whisper-tiny-mlx",
    "base": "mlx-community/whisper-base-mlx",
    "small": "mlx-community/whisper-small-mlx",
    "medium": "mlx-community/whisper-medium-mlx",
    "large": "mlx-community/whisper-large-v3-mlx",
    "large-v3": "mlx-community/whisper-large-v3-mlx",
    "large-v3-turbo": "mlx-community/whisper-large-v3-turbo",
}


def model_name() -> str:
    override = os.environ.get("WHISPR_MLX_MODEL", "").strip()
    if override:
        return override
    configured = os.environ.get("WHISPR_WHISPER_MODEL", "small").strip()
    return _MODEL_ALIASES.get(configured, configured)


class WhisperMlxSession(SttSession):
    def __init__(self, language: str, dictionary: list[str] | None = None) -> None:
        super().__init__()
        self.language = None if language in ("", "auto") else language
        self.initial_prompt = ", ".join(dictionary or []) or None
        self.model = model_name()
        self.chunks: list = []
        self.samples = 0
        self.decoded_samples = 0
        self.last_partial = ""
        self.stopped = asyncio.Event()
        self.task: asyncio.Task | None = None

    async def start(self) -> None:
        # mlx-whisper keeps the loaded model in its process-wide ModelHolder.
        # The first partial performs the initial download/load without blocking
        # gateway startup or allocating GPU memory before it is needed.
        logger.info("MLX Whisper ready to load %s on Apple GPU", self.model)
        self.task = asyncio.create_task(self._partial_loop())

    async def feed(self, pcm: bytes) -> None:
        import numpy as np

        audio = np.frombuffer(pcm, dtype=np.int16).astype(np.float32) / 32768.0
        self.chunks.append(audio)
        self.samples += audio.size

    def _audio(self):
        import numpy as np

        if not self.chunks:
            return np.zeros(0, dtype=np.float32)
        return np.concatenate(self.chunks)

    def _transcribe(self, audio) -> str:
        import mlx_whisper

        # MLX uses a shared model cache. Serialize decodes so a live partial
        # cannot compete with another session for unified GPU memory.
        with _INFERENCE_LOCK:
            result = mlx_whisper.transcribe(
                audio,
                path_or_hf_repo=self.model,
                language=self.language,
                initial_prompt=self.initial_prompt,
                condition_on_previous_text=False,
                fp16=True,
                verbose=None,
            )
        return result.get("text", "").strip()

    async def _partial_loop(self) -> None:
        while not self.stopped.is_set():
            try:
                await asyncio.wait_for(
                    self.stopped.wait(), timeout=PARTIAL_INTERVAL_S
                )
                break
            except asyncio.TimeoutError:
                pass
            if (
                self.samples <= self.decoded_samples
                or self.samples < SAMPLE_RATE * MIN_PARTIAL_AUDIO_S
            ):
                continue
            self.decoded_samples = self.samples
            try:
                text = await asyncio.to_thread(self._transcribe, self._audio())
            except Exception:
                logger.exception("MLX partial decode failed")
                continue
            if text and text != self.last_partial:
                self.last_partial = text
                await self.partial_queue.put(text)

    async def finish(self) -> str:
        self.stopped.set()
        if self.task is not None:
            await self.task
        final = ""
        if self.samples:
            try:
                final = await asyncio.to_thread(self._transcribe, self._audio())
            except Exception:
                logger.exception("MLX final decode failed; using last partial")
                final = self.last_partial
        await self.partial_queue.put(None)
        return final
