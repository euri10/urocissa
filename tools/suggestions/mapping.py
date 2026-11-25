"""Frozen Florence-2 candidate mapping from the 2026-11-25 public screen.

The model's raw text is private. Callers must not log inputs or return them to
the gallery; only the reviewed canonical labels leave this module.
"""
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent
PARENTS = json.loads((ROOT / "parents.json").read_text(encoding="utf-8"))
EXCLUDED = {"flower", "rose", "sunflower", "ground"}
ALIASES = {}
for path in (ROOT / "allowed-labels.txt", ROOT / "extra-labels.txt"):
    for line in path.read_text(encoding="utf-8").splitlines():
        canonical, *spellings = line.split("|")
        if canonical in EXCLUDED:
            continue
        for spelling in [canonical, *spellings]:
            if spelling in ALIASES and ALIASES[spelling] != canonical:
                raise ValueError("ambiguous frozen alias")
            ALIASES[spelling] = canonical

PATTERN = re.compile(
    r"(?<![\w-])(?:"
    + "|".join(re.escape(a) for a in sorted(ALIASES, key=lambda x: (-len(x), x)))
    + r")(?![\w-])"
)
UNSAFE = re.compile(
    r"\b(?:no|not|never|without|neither|nor|maybe|perhaps|possibly|probably|might|may|could|seems?|appears?|text|words?|sign|logo|brand|drawing|painting|statue|toy)\b|n't\b|\?"
)


def candidates(od, dense, caption):
    """Apply the frozen ordered mapping and return at most eight labels."""
    phrases = od + dense + re.split(r"[.!?;\n]+", caption)
    result = []
    for phrase in phrases:
        phrase = phrase.lower().strip()
        if UNSAFE.search(phrase):
            continue
        found = [phrase] if phrase in ALIASES else [m.group() for m in PATTERN.finditer(phrase)]
        for spelling in found:
            name = ALIASES[spelling]
            if name not in result:
                result.append(name)

    suppressed = set()

    def mark(name):
        for parent in PARENTS.get(name, []):
            if parent not in suppressed:
                suppressed.add(parent)
                mark(parent)

    for name in result:
        mark(name)
    return [name for name in result if name not in suppressed][:8]
