import sys
from types import SimpleNamespace

import app.providers as providers
from app.providers import create_session, resolve_provider_name
from app.providers.whisper_local import _create_model
from app.providers.whisper_mlx import WhisperMlxSession, model_name


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
    monkeypatch.setattr(providers, "_is_apple_silicon", lambda: False)
    monkeypatch.setattr(
        providers, "_module_available", lambda name: name == "faster_whisper"
    )
    assert resolve_provider_name() == "whisper_local"


def test_apple_silicon_prefers_mlx(monkeypatch):
    monkeypatch.delenv("WHISPR_STT_PROVIDER", raising=False)
    monkeypatch.delenv("DEEPGRAM_API_KEY", raising=False)
    monkeypatch.setattr(providers, "_is_apple_silicon", lambda: True)
    monkeypatch.setattr(providers, "_module_available", lambda _name: True)
    assert resolve_provider_name() == "whisper_mlx"


def test_explicit_mlx_session_receives_dictionary(monkeypatch):
    monkeypatch.setenv("WHISPR_STT_PROVIDER", "whisper_mlx")
    session = create_session(language="ru", dictionary=["NovaWhisper"])
    assert isinstance(session, WhisperMlxSession)
    assert session.language == "ru"
    assert session.initial_prompt == "NovaWhisper"


def test_mlx_small_alias(monkeypatch):
    monkeypatch.delenv("WHISPR_MLX_MODEL", raising=False)
    monkeypatch.setenv("WHISPR_WHISPER_MODEL", "small")
    assert model_name() == "mlx-community/whisper-small-mlx"


def test_mlx_transcribe_uses_greedy_decoder_without_beam_search(monkeypatch):
    received = {}

    def transcribe(audio, **options):
        received.update(options)
        assert audio == "samples"
        return {"text": " GPU transcript "}

    monkeypatch.setitem(sys.modules, "mlx_whisper", SimpleNamespace(transcribe=transcribe))
    session = WhisperMlxSession("en", ["NovaWhisper"])

    assert session._transcribe("samples") == "GPU transcript"
    assert received["path_or_hf_repo"] == "mlx-community/whisper-small-mlx"
    assert received["initial_prompt"] == "NovaWhisper"
    assert "beam_size" not in received


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
