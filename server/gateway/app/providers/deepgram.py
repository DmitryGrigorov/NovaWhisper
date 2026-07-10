"""Deepgram streaming STT provider (wss://api.deepgram.com/v1/listen).

Requires DEEPGRAM_API_KEY. Model/language options can be tuned with
WHISPR_DEEPGRAM_MODEL (default nova-2).
"""

import asyncio
import json
import os
import urllib.parse

import websockets

from .base import SttSession

DG_URL = "wss://api.deepgram.com/v1/listen"


class DeepgramSession(SttSession):
    def __init__(self, language: str, keywords: list[str]) -> None:
        super().__init__()
        self.language = language
        self.keywords = keywords
        self.ws: websockets.WebSocketClientProtocol | None = None
        self.reader: asyncio.Task | None = None
        self.final_segments: list[str] = []
        self.interim = ""

    async def start(self) -> None:
        api_key = os.environ["DEEPGRAM_API_KEY"]
        params = {
            "encoding": "linear16",
            "sample_rate": "16000",
            "channels": "1",
            "interim_results": "true",
            "smart_format": "true",
            "punctuate": "true",
            "model": os.environ.get("WHISPR_DEEPGRAM_MODEL", "nova-2"),
        }
        if self.language and self.language != "auto":
            params["language"] = self.language
        else:
            params["detect_language"] = "true"
        query = urllib.parse.urlencode(params)
        if self.keywords:
            # keyword boosting uses one repeated query param per term
            query += "&" + "&".join(
                f"keywords={urllib.parse.quote(kw)}" for kw in self.keywords[:100]
            )
        self.ws = await websockets.connect(
            f"{DG_URL}?{query}",
            extra_headers={"Authorization": f"Token {api_key}"},
        )
        self.reader = asyncio.create_task(self._read_loop())

    async def _read_loop(self) -> None:
        assert self.ws is not None
        try:
            async for raw in self.ws:
                msg = json.loads(raw)
                if msg.get("type") != "Results":
                    continue
                alt = msg.get("channel", {}).get("alternatives", [{}])[0]
                transcript = alt.get("transcript", "")
                if not transcript:
                    continue
                if msg.get("is_final"):
                    self.final_segments.append(transcript)
                    self.interim = ""
                else:
                    self.interim = transcript
                text = " ".join(self.final_segments + ([self.interim] if self.interim else []))
                await self.partial_queue.put(text)
        except websockets.ConnectionClosed:
            pass
        finally:
            await self.partial_queue.put(None)

    async def feed(self, pcm: bytes) -> None:
        if self.ws is not None:
            await self.ws.send(pcm)

    async def finish(self) -> str:
        if self.ws is not None:
            try:
                await self.ws.send(json.dumps({"type": "CloseStream"}))
            except websockets.ConnectionClosed:
                pass
        if self.reader is not None:
            try:
                await asyncio.wait_for(self.reader, timeout=10)
            except asyncio.TimeoutError:
                self.reader.cancel()
        if self.ws is not None:
            await self.ws.close()
        return " ".join(self.final_segments)
