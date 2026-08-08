import json

from fastapi.testclient import TestClient

from app.main import MAX_AUDIO_FRAME_BYTES, app

START = {
    "type": "start",
    "audio": {"format": "pcm_s16le", "sample_rate": 16000, "channels": 1},
    "language": "auto",
    "dictionary": ["Whispr"],
    "polish": {"mode": "fillers"},
}


def test_stream_end_to_end(monkeypatch):
    monkeypatch.setenv("WHISPR_STT_PROVIDER", "mock")
    monkeypatch.setenv(
        "WHISPR_MOCK_TRANSCRIPT", "um hello world this is whispr speaking"
    )
    client = TestClient(app)
    with client.websocket_connect("/v1/stream") as ws:
        ws.send_text(json.dumps(START))
        assert ws.receive_json() == {"type": "ready"}

        # 4 seconds of silence in 100ms chunks -> mock reveals ~10 words.
        chunk = b"\x00\x00" * 1600
        for _ in range(40):
            ws.send_bytes(chunk)
        ws.send_text(json.dumps({"type": "stop"}))

        partials = []
        final = None
        while final is None:
            msg = ws.receive_json()
            if msg["type"] == "partial":
                partials.append(msg["text"])
            elif msg["type"] == "final":
                final = msg

    assert partials, "expected at least one partial transcript"
    assert final["raw_text"] == "um hello world this is whispr speaking"
    assert final["text"] == "Hello world this is Whispr speaking."
    assert final["duration_ms"] >= 0


def test_rejects_bad_first_message(monkeypatch):
    monkeypatch.setenv("WHISPR_STT_PROVIDER", "mock")
    client = TestClient(app)
    with client.websocket_connect("/v1/stream") as ws:
        ws.send_text(json.dumps({"type": "stop"}))
        msg = ws.receive_json()
        assert msg["type"] == "error"


def test_rejects_invalid_dictionary(monkeypatch):
    monkeypatch.setenv("WHISPR_STT_PROVIDER", "mock")
    client = TestClient(app)
    with client.websocket_connect("/v1/stream") as ws:
        invalid = {**START, "dictionary": "not-a-list"}
        ws.send_text(json.dumps(invalid))
        assert ws.receive_json() == {"type": "error", "message": "invalid dictionary"}


def test_rejects_oversized_audio_frame(monkeypatch):
    monkeypatch.setenv("WHISPR_STT_PROVIDER", "mock")
    client = TestClient(app)
    with client.websocket_connect("/v1/stream") as ws:
        ws.send_text(json.dumps(START))
        assert ws.receive_json() == {"type": "ready"}
        ws.send_bytes(b"\x00\x00" * (MAX_AUDIO_FRAME_BYTES // 2 + 1))
        assert ws.receive_json() == {"type": "error", "message": "invalid audio frame"}
