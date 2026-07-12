import importlib.util
import os
import platform

from .base import SttSession


def _module_available(name: str) -> bool:
    return importlib.util.find_spec(name) is not None


def _is_apple_silicon() -> bool:
    return platform.system() == "Darwin" and platform.machine() == "arm64"


def resolve_provider_name() -> str:
    """Select the best installed backend without crossing platform stacks."""
    explicit = os.environ.get("WHISPR_STT_PROVIDER", "").lower()
    if explicit:
        return explicit
    if os.environ.get("DEEPGRAM_API_KEY"):
        return "deepgram"
    if _is_apple_silicon() and _module_available("mlx_whisper"):
        return "whisper_mlx"
    if _module_available("faster_whisper"):
        return "whisper_local"
    return "mock"


def create_session(language: str, dictionary: list[str]) -> SttSession:
    provider = resolve_provider_name()
    if provider == "deepgram":
        from .deepgram import DeepgramSession

        return DeepgramSession(language=language, keywords=dictionary)
    if provider == "whisper_local":
        from .whisper_local import WhisperLocalSession

        return WhisperLocalSession(language=language)
    if provider == "whisper_mlx":
        from .whisper_mlx import WhisperMlxSession

        return WhisperMlxSession(language=language, dictionary=dictionary)
    from .mock import MockSession

    return MockSession()
