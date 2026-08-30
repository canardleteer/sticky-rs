#!/usr/bin/env python3
"""Populate the local (gitignored) datasheet cache for this skill.

Downloads vendor PDFs into resources/datasheets/pdf/ and extracts Markdown
into resources/datasheets/md/. Records SHA-256 of every cached file in
resources/datasheets.sha256 (committed) so an IPFS CIDv1 can be derived later.

This is a machine-local cache, not a vendored documentation corpus.
Agents must not run `fetch` unless a human asked. `status` is local-only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

SKILL_DIR = Path(__file__).resolve().parent.parent
RESOURCES_DIR = SKILL_DIR / "resources"
CACHE_DIR = RESOURCES_DIR / "datasheets"
PDF_DIR = CACHE_DIR / "pdf"
MD_DIR = CACHE_DIR / "md"
SHA256_PATH = RESOURCES_DIR / "datasheets.sha256"
SHA256_JSON_PATH = RESOURCES_DIR / "datasheets.sha256.json"

USER_AGENT = (
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36"
)

# Keep ids and filenames in sync with resources/datasheets.md.
# urls are tried in order; later entries are GitHub or other public mirrors.
DOCUMENTS: tuple[dict[str, object], ...] = (
    {
        "id": "ssd1677",
        "title": "Solomon Systech SSD1677 datasheet Rev 1.0",
        "urls": (
            "https://files.waveshare.com/upload/2/2a/SSD1677_1.0.pdf",
            "https://www.solumco.com/files/SSD1677.pdf",
        ),
    },
    {
        "id": "bq27220-sluscb7",
        "title": "TI BQ27220 datasheet SLUSCB7",
        "urls": (
            "https://www.ti.com/lit/ds/symlink/bq27220.pdf",
            "https://www.ti.com/lit/pdf/sluscb7",
            "https://github.com/kodediy/kode_bq27220-idf/raw/main/BQ27220_Datasheet_RevA.pdf",
        ),
    },
    {
        "id": "bq27220-sluubd4",
        "title": "TI BQ27220 technical reference SLUUBD4",
        "urls": (
            "https://www.ti.com/lit/ug/sluubd4/sluubd4.pdf",
            "https://www.ti.com/lit/pdf/sluubd4",
        ),
    },
    {
        "id": "bq25616",
        "title": "TI BQ25616 datasheet SLUSDF7",
        "urls": (
            "https://www.ti.com/lit/ds/symlink/bq25616.pdf",
            "https://www.ti.com/lit/pdf/slusdf7",
        ),
    },
    {
        "id": "lsm6ds3tr-c",
        "title": "ST LSM6DS3TR-C datasheet",
        "urls": (
            "https://www.st.com/resource/en/datasheet/lsm6ds3tr-c.pdf",
            "https://www.makerguides.com/wp-content/uploads/2025/09/lsm6ds3tr-c-datasheet.pdf",
        ),
    },
    {
        "id": "gt911",
        "title": "Goodix GT911 datasheet",
        "urls": (
            "https://files.waveshare.com/wiki/common/GT911_EN_Datasheet.pdf",
            "https://files.pine64.org/doc/datasheet/pine64/GT911%20Capacitive%20Touch%20Controller%20Datasheet.pdf",
        ),
    },
    {
        "id": "sht4x",
        "title": "Sensirion SHT4x datasheet",
        "urls": (
            "https://sensirion.com/media/documents/33FD6951/6A7C10A0/HT_DS_Datasheet_SHT4x_V7.3.pdf",
            "https://sensirion.com/resource/datasheet/sht4x",
            "https://sensirion.com/media/documents/33FD6951/67EB9032/HT_DS_Datasheet_SHT4x_5.pdf",
        ),
    },
    {
        "id": "pcf8563",
        "title": "NXP PCF8563 datasheet",
        "urls": (
            "https://www.nxp.com/docs/en/data-sheet/PCF8563.pdf",
            "https://datasheet.chipsfind.com/PCF8563T-F4-112-436673.pdf",
            "https://www.ethernut.de/elektor/hardware/datasheets/PCF8563_5.pdf",
        ),
    },
    {
        "id": "esp32-s3-datasheet",
        "title": "Espressif ESP32-S3 datasheet",
        "urls": (
            "https://documentation.espressif.com/esp32-s3_datasheet_en.pdf",
            "https://www.espressif.com/documentation/esp32-s3_datasheet_en.pdf",
            "https://www.espressif.com/sites/default/files/documentation/esp32-s3_datasheet_en.pdf",
        ),
    },
    {
        "id": "esp32-s3-trm",
        "title": "Espressif ESP32-S3 technical reference manual",
        "urls": (
            "https://documentation.espressif.com/esp32-s3_technical_reference_manual_en.pdf",
            "https://www.espressif.com/sites/default/files/documentation/esp32-s3_technical_reference_manual_en.pdf",
            "https://www.espressif.com/documentation/esp32-s3_technical_reference_manual_en.pdf",
        ),
    },
    {
        "id": "seeed-sticky-schematic",
        "title": "Seeed reTerminal Sticky schematic Rev 01",
        "urls": (
            "https://files.seeedstudio.com/wiki/reterminal_sticky/res/reTerminal_Sticky_Schematic_diagram_260609.pdf",
        ),
    },
)


def pdf_path(doc_id: str) -> Path:
    return PDF_DIR / f"{doc_id}.pdf"


def md_path(doc_id: str) -> Path:
    return MD_DIR / f"{doc_id}.md"


def cache_rel(path: Path) -> str:
    return str(path.relative_to(RESOURCES_DIR))


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def selected(doc_id: str | None) -> tuple[dict[str, object], ...]:
    if doc_id is None:
        return DOCUMENTS
    for doc in DOCUMENTS:
        if doc["id"] == doc_id:
            return (doc,)
    known = ", ".join(str(doc["id"]) for doc in DOCUMENTS)
    raise SystemExit(f"unknown id {doc_id!r}; expected one of: {known}")


def load_previous_hash_records() -> dict[str, dict[str, object]]:
    if not SHA256_JSON_PATH.is_file():
        return {}
    try:
        payload = json.loads(SHA256_JSON_PATH.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}
    previous: dict[str, dict[str, object]] = {}
    for record in payload.get("files", []):
        if isinstance(record, dict) and "path" in record:
            previous[str(record["path"])] = record
    return previous


def write_hashes() -> None:
    previous = load_previous_hash_records()
    records: list[dict[str, object]] = []
    lines: list[str] = []
    for doc in DOCUMENTS:
        doc_id = str(doc["id"])
        for path in (pdf_path(doc_id), md_path(doc_id)):
            rel = cache_rel(path)
            if path.is_file():
                digest = sha256_file(path)
                records.append(
                    {
                        "id": doc_id,
                        "path": rel,
                        "sha256": digest,
                        "bytes": path.stat().st_size,
                    }
                )
            elif rel in previous:
                # `fetch --id` must not drop hashes for files that are not
                # on this machine.
                records.append(previous[rel])
            else:
                continue
            lines.append(f"{records[-1]['sha256']}  {rel}\n")
    SHA256_PATH.write_text("".join(lines), encoding="utf-8")
    SHA256_JSON_PATH.write_text(
        json.dumps(
            {
                "algorithm": "sha256",
                "ipfs_note": (
                    "CIDv1 can be derived later from these SHA-256 digests "
                    "(typically raw codec 0x55 + sha2-256)."
                ),
                "files": records,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"wrote {SHA256_PATH.relative_to(SKILL_DIR)} ({len(records)} files)")


def load_expected_hashes() -> dict[str, str]:
    if not SHA256_PATH.is_file():
        return {}
    expected: dict[str, str] = {}
    for line in SHA256_PATH.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        digest, path = line.split(None, 1)
        expected[path] = digest
    return expected


def cmd_status(doc_id: str | None) -> int:
    missing: list[str] = []
    expected = load_expected_hashes()
    print(f"cache: {CACHE_DIR}")
    for doc in selected(doc_id):
        doc_id_s = str(doc["id"])
        flags = []
        for kind, path in (("pdf", pdf_path(doc_id_s)), ("md", md_path(doc_id_s))):
            if not path.is_file():
                flags.append(f"{kind}=NO")
                missing.append(f"{doc_id_s}:{kind}")
                continue
            digest = sha256_file(path)
            rel = cache_rel(path)
            want = expected.get(rel)
            if want and want != digest:
                flags.append(f"{kind}=HASH_MISMATCH")
                missing.append(f"{doc_id_s}:{kind}:hash")
            else:
                flags.append(f"{kind}=yes")
        print(f"{doc_id_s:22} {' '.join(flags)}")
    if missing:
        print()
        print("Cache incomplete or hash mismatch. Ask the user to capture files:")
        print(f"  python3 {Path(__file__).resolve()} fetch")
        return 1
    print("all listed datasheets are present (pdf + md)")
    return 0


def download_url(url: str) -> bytes:
    parsed_host = urllib.parse.urlparse(url).netloc
    request = urllib.request.Request(
        url,
        headers={
            "User-Agent": USER_AGENT,
            "Accept": "application/pdf,application/octet-stream,*/*;q=0.8",
            "Referer": f"https://{parsed_host}/",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.read()
    except TimeoutError as error:
        raise urllib.error.URLError(f"timeout: {error}") from error


def download(doc: dict[str, object]) -> str:
    dest = pdf_path(str(doc["id"]))
    dest.parent.mkdir(parents=True, exist_ok=True)
    errors: list[str] = []
    for url in doc["urls"]:  # type: ignore[union-attr]
        try:
            body = download_url(str(url))
        except urllib.error.URLError as error:
            errors.append(f"{url}: {error}")
            continue
        if not body.startswith(b"%PDF"):
            errors.append(f"{url}: not a PDF")
            continue
        if len(body) < 8_000:
            errors.append(f"{url}: too small ({len(body)} bytes)")
            continue
        dest.write_bytes(body)
        print(f"wrote {dest.relative_to(SKILL_DIR)} ({len(body)} bytes) from {url}")
        return str(url)
    raise RuntimeError(
        f"{doc['id']}: download failed. Save the file as {dest} and run convert.\n"
        + "\n".join(errors)
    )


def convert_one(doc: dict[str, object], source_url: str | None) -> None:
    source = pdf_path(str(doc["id"]))
    dest = md_path(str(doc["id"]))
    if not source.is_file():
        raise RuntimeError(f"{doc['id']}: missing {source}")
    pdftotext = shutil.which("pdftotext")
    if pdftotext is None:
        raise RuntimeError(
            "pdftotext not found (install poppler-utils). "
            f"PDF is at {source}; markdown was not written."
        )
    result = subprocess.run(
        [pdftotext, "-layout", "-enc", "UTF-8", str(source), "-"],
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        err = result.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(f"{doc['id']}: pdftotext failed: {err or result.returncode}")
    text = result.stdout.decode("utf-8", errors="replace").strip()
    dest.parent.mkdir(parents=True, exist_ok=True)
    digest = sha256_file(source)
    used = source_url or str(doc["urls"][0])  # type: ignore[index]
    header = (
        f"# {doc['title']}\n\n"
        f"- id: `{doc['id']}`\n"
        f"- source: {used}\n"
        f"- local pdf: `pdf/{doc['id']}.pdf`\n"
        f"- pdf sha256: `{digest}`\n"
        f"- extracted with `pdftotext -layout` for agent reading; figures stay in the PDF\n\n"
        "---\n\n"
    )
    dest.write_text(header + text + "\n", encoding="utf-8")
    print(f"wrote {dest.relative_to(SKILL_DIR)}")


def cmd_fetch(doc_id: str | None, force: bool) -> int:
    failed = 0
    for doc in selected(doc_id):
        dest = pdf_path(str(doc["id"]))
        source_url: str | None = None
        if dest.is_file() and not force:
            print(f"{doc['id']}: pdf already present")
        else:
            try:
                source_url = download(doc)
            except RuntimeError as error:
                print(error, file=sys.stderr)
                failed += 1
                continue
        try:
            convert_one(doc, source_url)
        except RuntimeError as error:
            print(error, file=sys.stderr)
            failed += 1
    write_hashes()
    return 1 if failed else 0


def cmd_convert(doc_id: str | None) -> int:
    failed = 0
    for doc in selected(doc_id):
        try:
            convert_one(doc, None)
        except RuntimeError as error:
            print(error, file=sys.stderr)
            failed += 1
    write_hashes()
    return 1 if failed else 0


def cmd_hash() -> int:
    write_hashes()
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Local datasheet cache for seeed-sticky-hardware (gitignored)."
    )
    parser.add_argument(
        "command",
        choices=("status", "fetch", "convert", "hash"),
        help="status is local-only; fetch downloads (needs a human ask)",
    )
    parser.add_argument("--id", dest="doc_id", help="limit to one document id")
    parser.add_argument(
        "--force",
        action="store_true",
        help="re-download PDFs even when already present",
    )
    args = parser.parse_args()
    if args.command == "status":
        return cmd_status(args.doc_id)
    if args.command == "fetch":
        return cmd_fetch(args.doc_id, args.force)
    if args.command == "hash":
        return cmd_hash()
    return cmd_convert(args.doc_id)


if __name__ == "__main__":
    sys.exit(main())
