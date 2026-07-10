#!/usr/bin/env python3
# Thin generator/verifier for the Korean recall eval corpus.
#
# The two JSONL files (graph_korean_corpus.jsonl, graph_korean_queries.jsonl)
# are the source of truth and are checked in. This script *verifies* their
# invariants rather than regenerating them, so a reviewer can audit the data
# directly. Run: python3 build_korean_corpus.py
#
# Invariants checked:
#   1. All strings are NFC-normalised Korean (NFD would break both search paths).
#   2. Every query's expected_ids reference real corpus documents.
#   3. Document importance is strictly descending by id (deterministic tie-break;
#      both search paths rank by importance DESC).
#   4. No corpus body contains a stray control char / null byte.

import json
import sys
import unicodedata
from pathlib import Path

HERE = Path(__file__).parent


def load_jsonl(name):
    docs = []
    with open(HERE / name, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                docs.append(json.loads(line))
    return docs


def assert_nfc(text, where):
    if unicodedata.normalize("NFC", text) != text:
        raise SystemExit(f"NFC violation in {where}: {text!r}")


def main():
    corpus = load_jsonl("graph_korean_corpus.jsonl")
    queries = load_jsonl("graph_korean_queries.jsonl")
    ids = {d["id"] for d in corpus}

    # 1. NFC check
    for d in corpus:
        for field in ("id", "node_type", "title", "body"):
            assert_nfc(d[field], f"corpus {d['id']}.{field}")
        for t in d["tags"]:
            assert_nfc(t, f"corpus {d['id']}.tags")
    for q in queries:
        assert_nfc(q["query"], f"query {q['query']!r}")
        for eid in q["expected_ids"]:
            assert_nfc(eid, f"query {q['query']!r}.expected_ids")

    # 2. expected_ids exist
    for q in queries:
        for eid in q["expected_ids"]:
            if eid not in ids:
                raise SystemExit(f"unknown expected_id {eid!r} for query {q['query']!r}")

    # 3. strictly descending importance by id order (id == kd-NNN zero-padded)
    def num(d):
        return int(d["id"].split("-")[1])
    ordered = sorted(corpus, key=num)
    for a, b in zip(ordered, ordered[1:]):
        if not (a["importance"] > b["importance"]):
            raise SystemExit(
                f"importance not strictly descending: {a['id']}={a['importance']} "
                f"then {b['id']}={b['importance']}"
            )

    # 4. no control chars
    for d in corpus:
        for field in ("title", "body"):
            if any(ord(c) < 0x20 for c in d[field]):
                raise SystemExit(f"control char in {d['id']}.{field}")

    cats = {}
    for q in queries:
        cats[q["category"]] = cats.get(q["category"], 0) + 1
    print(f"OK: {len(corpus)} docs, {len(queries)} queries")
    for c, n in sorted(cats.items()):
        print(f"  {c}: {n}")


if __name__ == "__main__":
    main()
    sys.exit(0)
