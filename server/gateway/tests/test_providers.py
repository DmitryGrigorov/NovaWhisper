import importlib.util

from app.providers import resolve_provider_name
from app.providers.whisper_local import _create_model


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


def test_unsupported_cpu_compute_type_falls_back_to_int8():
    calls = []

    class FakeModel:
        def __init__(self, name, *, device, compute_type):
            calls.append((name, device, compute_type))
            if compute_type == "int8_float16":
                raise ValueError("Requested int8_float16 compute type, but unsupported")

    model = _create_model(FakeModel, "small", "cpu", "int8_float16")

    assert isinstance(model, FakeModel)
    assert calls == [
        ("small", "cpu", "int8_float16"),
        ("small", "cpu", "int8"),
    ]


def test_unrelated_model_error_is_not_hidden():
    class BrokenModel:
        def __init__(self, *_args, **_kwargs):
            raise ValueError("model files are corrupt")

    try:
        _create_model(BrokenModel, "small", "cpu", "int8")
    except ValueError as error:
        assert str(error) == "model files are corrupt"
    else:
        raise AssertionError("expected the model error to propagate")
