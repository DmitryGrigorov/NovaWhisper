"""Local Whisper STT via faster-whisper (CTranslate2) — runs on your own GPU.

No audio leaves the machine. On an RTX-class NVIDIA GPU with CUDA, large-v3
models transcribe many times faster than real time.

Env:
  WHISPR_WHISPER_MODEL    model name or CTranslate2 dir (default large-v3-turbo)
  WHISPR_WHISPER_DEVICE   cuda | cpu | auto (default auto)
  WHISPR_WHISPER_COMPUTE  float16 | int8_float16 | int8 | default (default: default)

Install: pip install -r requirements-local.txt  (see README for CUDA notes)
"""

import asyncio
import logging
import os
import threading

from .base import SttSession

logger = logging.getLogger("whispr.whisper_local")

SAMPLE_RATE = 16_000
PARTIAL_INTERVAL_S = 1.0
MIN_PARTIAL_AUDIO_S = 0.6

_MODELS: dict[tuple, object] = {}
_MODEL_LOCK = threading.Lock()
_DLL_HANDLES: list[object] = []


def _configure_windows_cuda_dlls() -> None:
    """Expose CUDA DLLs installed by the optional NVIDIA PyPI packages.

    Windows does not automatically search package-local ``bin`` directories,
    so CTranslate2 otherwise fails only when the first transcription begins.
    Keep the returned directory handles alive for the lifetime of the process.
    """
    if os.name != "nt" or not hasattr(os, "add_dll_directory"):
        return

    import nvidia

    nvidia_root = next(iter(nvidia.__path__))
    dll_dirs = []
    for component in ("cublas", "cudnn", "cuda_nvrtc"):
        dll_dir = os.path.join(nvidia_root, component, "bin")
        if os.path.isdir(dll_dir):
            dll_dirs.append(dll_dir)
            _DLL_HANDLES.append(os.add_dll_directory(dll_dir))

    # CTranslate2 resolves its CUDA dependencies with LoadLibrary, whose
    # legacy search path still consults PATH on Windows.
    if dll_dirs:
        current_path = os.environ.get("PATH", "")
        os.environ["PATH"] = os.pathsep.join([*dll_dirs, current_path])


def _model_config() -> tuple[str, str, str]:
    return (
        os.environ.get("WHISPR_WHISPER_MODEL", "large-v3-turbo"),
        os.environ.get("WHISPR_WHISPER_DEVICE", "auto"),
        os.environ.get("WHISPR_WHISPER_COMPUTE", "default"),
    )


def load_model():
    """Load (and cache) the WhisperModel. Blocking — call off the event loop.
    The first call downloads the model weights."""
    _configure_windows_cuda_dlls()
    from faster_whisper import WhisperModel

    key = _model_config()
    with _MODEL_LOCK:
        if key not in _MODELS:
            name, device, compute = key
            logger.info("loading whisper model %s (device=%s, compute=%s)", name, device, compute)
            _MODELS[key] = WhisperModel(name, device=device, compute_type=compute)
            logger.info("whisper model ready")
        return _MODELS[key]


class WhisperLocalSession(SttSession):
    def __init__(self, language: str) -> None:
        super().__init__()
        self.language = None if language in ("", "auto") else language
        self.chunks: list = []
        self.samples = 0
        self.decoded_samples = 0
        self.last_partial = ""
        self.stopped = asyncio.Event()
        self.model = None
        self.task: asyncio.Task | None = None

    async def start(self) -> None:
        self.model = await asyncio.to_thread(load_model)
        self.task = asyncio.create_task(self._partial_loop())

    async def feed(self, pcm: bytes) -> None:
        import numpy as np

        arr = np.frombuffer(pcm, dtype=np.int16).astype(np.float32) / 32768.0
        self.chunks.append(arr)
        self.samples += arr.size

    def _audio(self):
        import numpy as np

        if not self.chunks:
            return np.zeros(0, dtype=np.float32)
        return np.concatenate(self.chunks)

    def _transcribe(self, audio, beam_size: int) -> str:
        segments, _info = self.model.transcribe(
            audio,
            language=self.language,
            beam_size=beam_size,
            vad_filter=True,
            condition_on_previous_text=False,
        )
        return " ".join(s.text.strip() for s in segments).strip()

    async def _partial_loop(self) -> None:
        """Re-decode the growing utterance every second for live partials.
        Fast (beam 1) decodes; the final pass in finish() uses a wider beam."""
        while not self.stopped.is_set():
            try:
                await asyncio.wait_for(self.stopped.wait(), timeout=PARTIAL_INTERVAL_S)
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
                text = await asyncio.to_thread(self._transcribe, self._audio(), 1)
            except Exception:
                logger.exception("partial decode failed")
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
                final = await asyncio.to_thread(self._transcribe, self._audio(), 5)
            except Exception:
                logger.exception("final decode failed; using last partial")
                final = self.last_partial
        await self.partial_queue.put(None)
        return final
