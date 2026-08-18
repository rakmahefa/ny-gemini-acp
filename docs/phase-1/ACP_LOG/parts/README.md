# Split manifest — `docs/phase-1/ACP_LOG.md`

- Source JSON documents: 1
- Flattened events: 455
- Events per part: 100
- Parts contain complete top-level events and preserve source order.
- Parsing uses JSON `strict=False` because captured ACP/tool text may contain raw control characters.
- Multiple concatenated top-level JSON documents are accepted and flattened.

- `part-0001.json` — events 1..100
- `part-0002.json` — events 101..200
- `part-0003.json` — events 201..300
- `part-0004.json` — events 301..400
- `part-0005.json` — events 401..455
