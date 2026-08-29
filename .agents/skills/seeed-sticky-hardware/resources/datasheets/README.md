# Local datasheet cache

Gitignored copies of vendor PDFs and text extracted for agents. The catalog
is [datasheets.md](../datasheets.md). Nothing in `pdf/` or `md/` is committed.
SHA-256 of those files is committed next to the catalog
([datasheets.sha256](../datasheets.sha256),
[datasheets.sha256.json](../datasheets.sha256.json)) so an IPFS CIDv1 can
be derived later.

| Path | Contents |
| --- | --- |
| `pdf/<id>.pdf` | Vendor file, as downloaded or saved by the user |
| `md/<id>.md` | `pdftotext -layout` extraction with a short header |

Expected `<id>` values are the Id column in [datasheets.md](../datasheets.md).

Populate this cache when the work is registers, opcodes, or timings. `status`
is local-only; do not `fetch` unless the user asked.

```shell
# from this skill directory
python3 scripts/fetch_datasheets.py status
python3 scripts/fetch_datasheets.py fetch
python3 scripts/fetch_datasheets.py convert
python3 scripts/fetch_datasheets.py hash
```

If a vendor site blocks the script, save that PDF into `pdf/` under the
filename `status` prints, then run `convert`.
