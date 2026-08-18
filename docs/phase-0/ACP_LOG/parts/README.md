# Split manifest — `docs/phase-0/ACP_LOG.md`

- Source JSON documents: 1
- Flattened events: 81
- Events per part: 100
- Parts contain complete top-level events and preserve source order.
- Parsing uses JSON `strict=False` because captured ACP/tool text may contain raw control characters.
- Multiple concatenated top-level JSON documents are accepted and flattened.

- `part-0001.json` — events 1..81
