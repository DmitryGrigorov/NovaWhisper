"""Transcript polishing.

Modes:
  none    — raw transcript passthrough
  fillers — rule-based: drop filler words, collapse stutters, fix spacing,
            capitalization, terminal punctuation, apply dictionary casing (P0)
  full    — LLM rewrite via the Claude API (falls back to `fillers` rules when
            no ANTHROPIC_API_KEY is configured or the call fails) (P1)
"""

import os
import re

# Conservative filler list: only tokens that are near-unambiguous disfluencies.
FILLER_RE = re.compile(
    r"\b(?:um+|uh+|uhm+|erm+|ehm+|hmm+|mhm+|ah+h*)\b[,.!?]?\s*",
    re.IGNORECASE,
)
DUP_WORD_RE = re.compile(r"\b(\w+)(\s+\1\b)+", re.IGNORECASE)

_llm_client = None


def apply_rules(text: str, dictionary: list[str] | None = None) -> str:
    out = FILLER_RE.sub("", text)
    out = DUP_WORD_RE.sub(r"\1", out)
    out = re.sub(r"\s+", " ", out).strip()
    out = re.sub(r"\s+([,.!?;:])", r"\1", out)
    if not out:
        return out

    for term in dictionary or []:
        out = re.sub(rf"\b{re.escape(term)}\b", term, out, flags=re.IGNORECASE)

    # Sentence-start capitalization.
    def cap(match: re.Match) -> str:
        return match.group(1) + match.group(2).upper()

    out = out[0].upper() + out[1:]
    out = re.sub(r"([.!?]\s+)(\w)", cap, out)
    if out[-1] not in ".!?…:,;":
        out += "."
    return out


POLISH_SYSTEM = """You clean up dictated speech into polished written text.
Rules:
- Remove filler words, false starts, and stutters.
- Fix grammar, punctuation, capitalization, and paragraph breaks.
- Preserve the speaker's meaning, tone, and language; do not add content.
- Handle mixed-language input by keeping each part in its original language.
- Apply spoken formatting commands ("new line", "new paragraph").
- Output ONLY the cleaned text, with no preamble or commentary."""


async def llm_polish(
    text: str,
    dictionary: list[str] | None = None,
    style: str | None = None,
    app: str | None = None,
) -> str:
    """Full LLM polish; falls back to rules on any failure."""
    fallback = apply_rules(text, dictionary)
    if not text.strip() or not os.environ.get("ANTHROPIC_API_KEY"):
        return fallback
    try:
        global _llm_client
        if _llm_client is None:
            import anthropic

            _llm_client = anthropic.AsyncAnthropic()

        context_lines = []
        if dictionary:
            context_lines.append(
                "Preferred spellings for these terms: " + ", ".join(dictionary)
            )
        if style:
            context_lines.append(f"Target tone: {style}")
        if app:
            context_lines.append(f"The user is typing in: {app}")
        context = ("\n".join(context_lines) + "\n\n") if context_lines else ""

        response = await _llm_client.messages.create(
            model=os.environ.get("WHISPR_POLISH_MODEL", "claude-haiku-4-5"),
            max_tokens=1024,
            system=[
                {
                    "type": "text",
                    "text": POLISH_SYSTEM,
                    # Engages once the static prefix crosses the model's
                    # minimum cacheable size (grows with the system prompt).
                    "cache_control": {"type": "ephemeral"},
                }
            ],
            messages=[{"role": "user", "content": f"{context}Transcript:\n{text}"}],
            timeout=10.0,
        )
        if response.stop_reason == "refusal":
            return fallback
        polished = next(
            (b.text for b in response.content if b.type == "text"), ""
        ).strip()
        return polished or fallback
    except Exception:
        return fallback


async def polish(
    text: str,
    mode: str,
    dictionary: list[str] | None = None,
    style: str | None = None,
    app: str | None = None,
) -> str:
    if mode == "none":
        return text
    if mode == "full":
        return await llm_polish(text, dictionary, style, app)
    return apply_rules(text, dictionary)
