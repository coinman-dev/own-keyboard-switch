#!/usr/bin/env python3
"""Prepare exact lowercase English targets from a local switching report."""

import argparse
import csv
import hashlib
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parents[1]
                        / "data/generated/extra-short-en.txt")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    raw = args.report.read_bytes()
    rows = csv.DictReader((line for line in raw.decode("utf-8-sig").splitlines()
                           if not line.startswith("#")), delimiter="\t")
    words = sorted({row["word"] for row in rows if row["result"] == "missed"})
    if not words or any(len(w) not in (2, 3) or not w.isascii()
                        or not w.isalpha() or not w.islower() for w in words):
        raise ValueError("Expected nonempty lowercase English words of 2 or 3 letters")
    content = ("# Additional English targets from the user-supplied short-word report.\n"
               "# Exact matches only; runtime Russian-word and name guards still apply.\n"
               f"# Report SHA-256: {hashlib.sha256(raw).hexdigest()}\n"
               + "\n".join(words) + "\n").encode("utf-8")
    if args.check:
        if args.output.read_bytes() != content:
            raise ValueError("Generated target list differs")
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(content)
    print(f"Verified {len(words)} exact lowercase targets")


if __name__ == "__main__":
    main()
