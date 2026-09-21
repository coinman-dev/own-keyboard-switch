#!/usr/bin/env python3
"""Build the embedded extra layout rules from the supplied source database."""

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import struct
import zlib

SOURCE_SHA256 = "36d3b1dd99663b5d58f102ce9edeaede8f99f1b07b19dd80156eeb48254d161f"
VERSION = "20170627"


def parse(raw):
    # Split physical lines BEFORE XOR. Decoded patterns can contain CR.
    lines = [bytes(b ^ 0xAA for b in line).decode("cp1251")
             for line in re.split(rb"\r\n|\r|\n", raw)]
    if lines[0] != "PSVersion=" + VERSION:
        raise ValueError("Unexpected database version")
    records = []
    for number, line in enumerate(lines[1:], 2):
        if not line:
            continue
        flags, pattern = "", line
        if line.startswith("_") and " " in line:
            flags, pattern = line[1:].split(" ", 1)
        if set(flags) - set("BPCEAD"):
            raise ValueError(f"Unknown flags at line {number}")
        mask = 1 if "P" in flags else 8 if "B" in flags else 2
        for flag, bit in (("C", 4), ("E", 16), ("A", 32), ("D", 64)):
            if flag in flags:
                mask |= bit
        records.append((number, mask, pattern))
    return records


def build(source):
    raw = source.read_bytes()
    if hashlib.sha256(raw).hexdigest() != SOURCE_SHA256:
        raise ValueError("Source SHA-256 differs from the studied database")
    records = parse(raw)
    assert len(records) == 37747
    counts = Counter(mask & 11 for _, mask, _ in records)
    assert counts == {1: 7324, 2: 16084, 8: 14339}
    # Retain source order, duplicate records and exact pattern bytes.
    payload = bytearray(b"OKBEXR01" + struct.pack("<I", len(records)))
    for number, mask, pattern in records:
        encoded = pattern.encode("utf-8")
        payload.extend(struct.pack("<IBI", number, mask, len(encoded)))
        payload.extend(encoded)
    compressed = zlib.compress(bytes(payload), level=9)
    metadata = {
        "format": "OKBEXR01", "database_version": VERSION,
        "source": "external layout-rule database",
        "source_sha256": SOURCE_SHA256, "records": len(records),
        "match_modes": {"equals": counts[1], "contains": counts[2], "starts_with": counts[8]},
        "uncompressed_bytes": len(payload), "compressed_bytes": len(compressed),
        "payload_sha256": hashlib.sha256(payload).hexdigest(),
        "compressed_sha256": hashlib.sha256(compressed).hexdigest(),
    }
    return compressed, metadata


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True, help="Path to the source rule database")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parents[1] / "data/generated")
    parser.add_argument("--check", action="store_true", help="Verify checked-in artifacts without writing")
    args = parser.parse_args()
    compressed, metadata = build(args.source)
    files = {"extra.rules.z": compressed,
             "extra.rules.json": (json.dumps(metadata, ensure_ascii=False, indent=2) + "\n").encode("utf-8")}
    for name, content in files.items():
        path = args.output / name
        if args.check:
            if path.read_bytes() != content:
                raise ValueError(f"Generated artifact differs: {name}")
        else:
            args.output.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
    print(f"Verified {metadata['records']} extra rules; {len(compressed)} compressed bytes")


if __name__ == "__main__":
    main()
