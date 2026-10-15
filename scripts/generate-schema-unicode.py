import argparse
import hashlib
import json
from pathlib import Path
import re

parser = argparse.ArgumentParser()
parser.add_argument("upstream_tables", type=Path)
args = parser.parse_args()
source = args.upstream_tables.read_bytes()
categories = {
    "UppercaseLetter": 1,
    "LowercaseLetter": 1,
    "TitlecaseLetter": 1,
    "ModifierLetter": 1,
    "OtherLetter": 1,
    "NonspacingMark": 2,
    "SpacingMark": 3,
    "EnclosingMark": 4,
    "DecimalNumber": 5,
}
values = bytearray(0x110000)
ranges = re.findall(rb"\((\d+), (\d+), GeneralCategory::(\w+)\)", source)
if not ranges:
    raise ValueError("input has no Unicode general-category ranges")
for start, end, name in ranges:
    category = categories.get(name.decode(), 0)
    if category:
        start, end = int(start), int(end)
        values[start:end + 1] = bytes([category]) * (end - start + 1)
last = len(values.rstrip(b"\0"))
page_size = 128
unique = {}
index = bytearray()
data = bytearray()
for offset in range(0, last, page_size):
    page = bytes(values[offset:offset + page_size])
    if page not in unique:
        unique[page] = len(unique)
        data.extend(page)
    index.append(unique[page])
destination = Path(__file__).resolve().parents[1] / "crates/core/schema/src"
(destination / "unicode-index.bin").write_bytes(index)
(destination / "unicode-categories.bin").write_bytes(data)
print(json.dumps({
    "input_sha256": hashlib.sha256(source).hexdigest(),
    "page_size": page_size,
    "unique_pages": len(unique),
    "index_bytes": len(index),
    "category_bytes": len(data),
    "index_sha256": hashlib.sha256(index).hexdigest(),
    "category_sha256": hashlib.sha256(data).hexdigest(),
}, indent=2))
