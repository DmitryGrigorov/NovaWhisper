"""Development STT provider: no network, no API key.

Reveals words of a canned transcript at ~2.5 words per second of received
audio, so partials and finals flow through the whole pipeline exactly like a
real provider. Override the transcript with WHISPR_MOCK_TRANSCRIPT.
"""

import os

from .base import SttSession

DEFAULT_TRANSCRIPT = "um hello world this is uh a test of the whispr dictation pipeline"
SAMPLE_RATE = 16_000
SECONDS_PER_WORD = 0.4


class MockSession(SttSession):
    def __init__(self) -> None:
        super().__init__()
        self.words = os.environ.get("WHISPR_MOCK_TRANSCRIPT", DEFAULT_TRANSCRIPT).split()
        self.samples_received = 0
        self.words_shown = 0

    async def start(self) -> None:
        pass

    async def feed(self, pcm: bytes) -> None:
        self.samples_received += len(pcm) // 2
        seconds = self.samples_received / SAMPLE_RATE
        target = min(int(seconds / SECONDS_PER_WORD), len(self.words))
        if target > self.words_shown:
            self.words_shown = target
            await self.partial_queue.put(" ".join(self.words[:target]))

    async def finish(self) -> str:
        await self.partial_queue.put(None)
        return " ".join(self.words[: self.words_shown])
