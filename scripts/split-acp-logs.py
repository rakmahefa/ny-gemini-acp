#!/usr/bin/env python3
"""Split canonical ACP JSON logs into deterministic, analysis-sized chunks.

The source file remains the canonical evidence. Generated chunks are copies of
complete top-level ACP events, never line-based fragments of an event.

The captured logs are not guaranteed to be one strict JSON document: long Zed
sessions may append multiple JSON arrays/objects to the same markdown artifact,
and captured tool/output text may contain raw control characters. The parser
therefore accepts concatenated top-level JSON documents with ``strict=False``
and flattens arrays while preserving document/event order.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def parse_documents(raw: str, source: Path) -> list[Any]:
    decoder = json.JSONDecoder(strict=False)
    pos = 0
    documents: list[Any] = []

    while pos < len(raw):
        while pos < len(raw) and raw[pos].isspace():
            pos += 1
        if pos >= len(raw):
            break

        try:
            value, end = decoder.raw_decode(raw, pos)
        except json.JSONDecodeError as exc:
            raise ValueError(
                f"{source} is not parseable as concatenated ACP JSON: "
                f"line {exc.lineno}, column {exc.colno}, char {exc.pos}: {exc.msg}"
            ) from exc

        documents.append(value)
        pos = end

    if not documents:
        raise ValueError(f"{source} does not contain any JSON document")
    return documents


def parse_events(source: Path) -> tuple[list[Any], int]:
    raw = source.read_text(encoding="utf-8")
    documents = parse_documents(raw, source)

    events: list[Any] = []
    for document in documents:
        if isinstance(document, list):
            events.extend(document)
        else:
            events.append(document)

    return events, len(documents)


def split_log(source: Path, output_dir: Path, events_per_part: int) -> None:
    if events_per_part <= 0:
        raise ValueError("events_per_part must be > 0")

    events, document_count = parse_events(source)

    output_dir.mkdir(parents=True, exist_ok=True)
    for old in output_dir.glob("part-*.json"):
        old.unlink()

    manifest = [
        f"# Split manifest — `{source}`",
        "",
        f"- Source JSON documents: {document_count}",
        f"- Flattened events: {len(events)}",
        f"- Events per part: {events_per_part}",
        "- Parts contain complete top-level events and preserve source order.",
        "- Parsing uses JSON `strict=False` because captured ACP/tool text may contain raw control characters.",
        "- Multiple concatenated top-level JSON documents are accepted and flattened.",
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
