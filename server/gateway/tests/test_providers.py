import importlib.util

from app.providers import resolve_provider_name


def test_explicit_provider_wins(monkeypatch):
    monkeypatch.setenv("WHISPR_STT_PROVIDER", "whisper_local")
    monkeypatch.setenv("DEEPGRAM_API_KEY", "dg-key")
    assert resolve_provider_name() == "whisper_local"


def test_deepgram_selected_when_key_present(monkeypatch):
    monkeypatch.delenv("WHISPR_STT_PROVIDER", raising=False)
    monkeypatch.setenv("DEEPGRAM_API_KEY", "dg-key")
    assert resolve_provider_name() == "deepgram"


def test_fallback_without_key(monkeypatch):
    monkeypatch.delenv("WHISPR_STT_PROVIDER", raising=False)
    monkeypatch.delenv("DEEPGRAM_API_KEY", raising=False)
    expected = (
        "whisper_local"
        if importlib.util.find_spec("faster_whisper") is not None
        else "mock"
    )
    assert resolve_provider_name() == expected
