import importlib.util
import os

from .base import SttSession


def resolve_provider_name() -> str:
    """WHISPR_STT_PROVIDER=mock|deepgram|whisper_local wins; otherwise pick the
    best available: Deepgram if a key is set, local GPU/CPU Whisper if
    faster-whisper is installed, else the offline mock."""
    explicit = os.environ.get("WHISPR_STT_PROVIDER", "").lower()
    if explicit:
        return explicit
    if os.environ.get("DEEPGRAM_API_KEY"):
        return "deepgram"
    if importlib.util.find_spec("faster_whisper") is not None:
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
    from .mock import MockSession

    return MockSession()
