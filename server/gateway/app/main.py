"""Whispr gateway: WebSocket audio streaming -> STT -> polish.

Run:  uvicorn app.main:app --host 127.0.0.1 --port 8765
Env:  WHISPR_STT_PROVIDER=mock|deepgram|whisper_local|whisper_mlx,
      DEEPGRAM_API_KEY, ANTHROPIC_API_KEY,
      WHISPR_POLISH_MODEL, WHISPR_MOCK_TRANSCRIPT
"""

import asyncio
import contextlib
import json
import logging
import time

from fastapi import FastAPI, WebSocket, WebSocketDisconnect

from .polish import polish
from .providers import create_session, resolve_provider_name

logger = logging.getLogger("whispr.gateway")


@contextlib.asynccontextmanager
async def lifespan(_app: FastAPI):
    # Preload the local Whisper model so the first utterance isn't slow.
    if resolve_provider_name() == "whisper_local":
        from .providers.whisper_local import load_model

        try:
            await asyncio.to_thread(load_model)
        except Exception:
            logger.exception("failed to preload whisper model")
    yield


app = FastAPI(title="Whispr Gateway", version="0.1.0", lifespan=lifespan)


@app.get("/healthz")
async def healthz() -> dict:
    return {"status": "ok"}


@app.websocket("/v1/stream")
async def stream(ws: WebSocket) -> None:
    await ws.accept()
    session = None
    forwarder = None
    try:
        start_raw = await ws.receive_text()
        start = json.loads(start_raw)
        if start.get("type") != "start":
            await ws.send_json({"type": "error", "message": "first message must be start"})
            return

        audio = start.get("audio", {})
        if audio.get("format", "pcm_s16le") != "pcm_s16le" or audio.get("sample_rate", 16000) != 16000:
            await ws.send_json({"type": "error", "message": "unsupported audio format"})
            return

        language = start.get("language", "auto")
        dictionary = start.get("dictionary", [])
        polish_mode = start.get("polish", {}).get("mode", "fillers")
        context = start.get("context", {}) or {}

        session = create_session(language=language, dictionary=dictionary)
        await session.start()
        await ws.send_json({"type": "ready"})
        started = time.monotonic()

        async def forward_partials() -> None:
            while True:
                text = await session.partial_queue.get()
                if text is None:
                    return
                try:
                    await ws.send_json({"type": "partial", "text": text})
                except Exception:
                    return

        forwarder = asyncio.create_task(forward_partials())

        # Receive audio until the client sends stop or disconnects.
        while True:
            msg = await ws.receive()
            if msg.get("type") == "websocket.disconnect":
                raise WebSocketDisconnect(msg.get("code", 1000))
            if msg.get("bytes") is not None:
                await session.feed(msg["bytes"])
            elif msg.get("text") is not None:
                control = json.loads(msg["text"])
                if control.get("type") == "stop":
                    break

        raw_text = await session.finish()
        await forwarder

        polished = await polish(
            raw_text,
            mode=polish_mode,
            dictionary=dictionary,
            style=context.get("style"),
            app=context.get("app"),
        )
        await ws.send_json(
            {
                "type": "final",
                "text": polished,
                "raw_text": raw_text,
                "duration_ms": int((time.monotonic() - started) * 1000),
            }
        )
        await ws.close()
    except WebSocketDisconnect:
        logger.info("client disconnected mid-utterance")
    except Exception:
        logger.exception("stream session failed")
        try:
            await ws.send_json({"type": "error", "message": "internal error"})
            await ws.close()
        except Exception:
            pass
    finally:
        if forwarder is not None and not forwarder.done():
            forwarder.cancel()
        if session is not None:
            try:
                await session.finish()
            except Exception:
                pass
