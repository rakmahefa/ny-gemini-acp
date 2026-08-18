#!/usr/bin/env python3
"""Split canonical ACP JSON logs into deterministic, analysis-sized chunks.

The source file remains the canonical evidence. Generated chunks are copies of
complete top-level JSON events, never line-based fragments of an event.

ACP logs can contain raw control characters inside captured tool/output text.
Python's JSON decoder rejects those by default even though they are valid bytes
inside the captured log payload. We therefore decode with ``strict=False`` and
re-serialize the resulting events as canonical JSON.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def split_log(source: Path, output_dir: Path, events_per_part: int) -> None:
    raw = source.read_text(encoding="utf-8")
    try:
        events = json.loads(raw, strict=False)
    except json.JSONDecodeError as exc:
        raise ValueError(
            f"{source} is not parseable as an ACP JSON array: "
            f"line {exc.lineno}, column {exc.colno}, char {exc.pos}: {exc.msg}"
        ) from exc

    if not isinstance(events, list):
        raise ValueError(f"{source} must contain a top-level JSON array")
    if events_per_part <= 0:
        raise ValueError("events_per_part must be > 0")

    output_dir.mkdir(parents=True, exist_ok=True)
    for old in output_dir.glob("part-*.json"):
        old.unlink()

    manifest = [
        f"# Split manifest — {source}",
        "",
        f"- Source events: {len(events)}",
        f"- Events per part: {events_per_part}",
        "- Parts contain complete top-level events and preserve event order.",
        "- Source decoding uses JSON `strict=False` because captured ACP/tool text may contain raw control characters.",
        "",
    ]

    for start in range(0, len(events), events_per_part):
        part_no = start // events_per_part + 1
        part = events[start : start + events_per_part]
        name = f"part-{part_no:04d}.json"
        (output_dir / name).write_text(
            json.dumps(part, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
        manifest.append(
            f"- `{name}` — events {start + 1}..{start + len(part)}"
        )

    (output_dir / "README.md").write_text(
        "\n".join(manifest) + "\n", encoding="utf-8"
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--events-per-part", type=int, default=100)
    args = parser.parse_args()
    try:
        split_log(args.source, args.output, args.events_per_part)
    except (OSError, ValueError) as exc:
        parser.error(str(exc))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
