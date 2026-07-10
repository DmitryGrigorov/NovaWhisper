"""Streaming STT provider interface.

A session covers one utterance. Feed raw PCM (16 kHz mono s16le) with
`feed()`; growing best-guess transcripts appear on `partial_queue` (a `None`
sentinel marks the end); `finish()` flushes the provider and returns the final
raw transcript.
"""

import asyncio
from abc import ABC, abstractmethod


class SttSession(ABC):
    def __init__(self) -> None:
        self.partial_queue: asyncio.Queue[str | None] = asyncio.Queue()

    @abstractmethod
    async def start(self) -> None: ...

    @abstractmethod
    async def feed(self, pcm: bytes) -> None: ...

    @abstractmethod
    async def finish(self) -> str: ...
