# Split manifest — `docs/phase-2/ACP_LOG.md`

- Source JSON documents: 2
- Flattened events: 1692
- Events per part: 100
- Parts contain complete top-level events and preserve source order.
- Parsing uses JSON `strict=False` because captured ACP/tool text may contain raw control characters.
- Multiple concatenated top-level JSON documents are accepted and flattened.

- `part-0001.json` — events 1..100
- `part-0002.json` — events 101..200
- `part-0003.json` — events 201..300
- `part-0004.json` — events 301..400
- `part-0005.json` — events 401..500
- `part-0006.json` — events 501..600
- `part-0007.json` — events 601..700
- `part-0008.json` — events 701..800
- `part-0009.json` — events 801..900
- `part-0010.json` — events 901..1000
- `part-0011.json` — events 1001..1100
- `part-0012.json` — events 1101..1200
- `part-0013.json` — events 1201..1300
- `part-0014.json` — events 1301..1400
- `part-0015.json` — events 1401..1500
- `part-0016.json` — events 1501..1600
- `part-0017.json` — events 1601..1692
