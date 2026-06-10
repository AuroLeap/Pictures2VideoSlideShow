#!/usr/bin/env python3
"""Traceability join + orphan report for the UN->SR->LLR->TC registries.

Stack-agnostic reference implementation (Python 3, standard library only — no
pip installs). Drop it in a new repo as `scripts/trace.py` and wire it into the
check harness / CI. It is the generated "traceability matrix" referenced by
PROCESS.md: it never needs hand-maintaining.

Usage:
    python scripts/trace.py [--strict] [--require-verified] [--docs DIR]

Reads (relative to --docs, default "docs"):
    requirements/system-requirements.csv   (cols: SR-ID, UN-Refs, Verification, Status, ...)
    requirements/low-level-requirements.csv (cols: LLR-ID, SR-Refs, ...)
    test/test-cases.csv                     (cols: TC-ID, Verifies, ...)
    requirements/user-needs.md              (optional; UN-### ids scraped for UN->SR coverage)

Writes:
    test/report.md  — counts, the SR->LLR->TC matrix, and the orphan list.

Exit code: 0 normally; with --strict, 1 if any orphan (or, with
--require-verified, any status finding) exists — use in gates.

Orphan rules (the method rules are stated once, in process.md §4):
    - SR with no LLR (unless Verification is Analysis/Inspection — those have no
      code to decompose; Demonstration/Manual SRs still describe behavior the
      software implements, so they keep the LLR requirement)
    - SR with no TC (every SR needs ≥1 TC row regardless of method; for human
      methods the TC records the procedure with Automated=No)
    - LLR with no SR parent, or referencing an unknown SR
    - LLR with no TC
    - TC that verifies nothing, or references an unknown SR/LLR
    - UN with no SR (only when user-needs.md is present)
--require-verified adds the G3 status criterion:
    - SR with Verification=Test whose Status is not Verified
Placeholder example rows (ids ending in "-000") are ignored.
"""

import argparse
import csv
import re
import sys
from pathlib import Path


def load_csv(path):
    if not path.exists():
        return []
    with path.open(newline="", encoding="utf-8-sig") as f:
        return list(csv.DictReader(f))


def refs(value):
    """Split a multi-ref cell (';', ',' or whitespace separated) into ids."""
    return [t for t in re.split(r"[;,\s]+", (value or "").strip()) if t]


def is_example(rid):
    return rid.endswith("-000")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--strict", action="store_true", help="exit 1 if any orphan / status finding"
    )
    ap.add_argument(
        "--require-verified",
        action="store_true",
        help="G3 criterion: flag Verification=Test SRs not Status=Verified",
    )
    ap.add_argument("--docs", default="docs", help="docs directory (default: docs)")
    args = ap.parse_args()
    docs = Path(args.docs)

    srs = [
        r
        for r in load_csv(docs / "requirements" / "system-requirements.csv")
        if r.get("SR-ID") and not is_example(r["SR-ID"])
    ]
    llrs = [
        r
        for r in load_csv(docs / "requirements" / "low-level-requirements.csv")
        if r.get("LLR-ID") and not is_example(r["LLR-ID"])
    ]
    tcs = [
        r
        for r in load_csv(docs / "test" / "test-cases.csv")
        if r.get("TC-ID") and not is_example(r["TC-ID"])
    ]

    un_ids = set()
    un_md = docs / "requirements" / "user-needs.md"
    if un_md.exists():
        un_ids = {
            u
            for u in re.findall(r"\bUN-\d+\b", un_md.read_text(encoding="utf-8"))
            if not is_example(u)
        }

    sr_ids = {r["SR-ID"] for r in srs}
    llr_ids = {r["LLR-ID"] for r in llrs}
    llr_sr_refs = {x for r in llrs for x in refs(r.get("SR-Refs"))}
    tc_refs = {x for r in tcs for x in refs(r.get("Verifies"))}
    sr_un_refs = {x for r in srs for x in refs(r.get("UN-Refs"))}

    orphans = []
    for r in srs:
        sid = r["SR-ID"]
        analytic = r.get("Verification", "") in ("Analysis", "Inspection")
        if not analytic and sid not in llr_sr_refs:
            orphans.append(
                f"SR {sid} has no LLR (and Verification != Analysis/Inspection)"
            )
        if sid not in tc_refs:
            orphans.append(f"SR {sid} has no test (TC)")
        for u in refs(r.get("UN-Refs")):
            if un_ids and u not in un_ids:
                orphans.append(f"SR {sid} references unknown {u}")

    for r in llrs:
        lid = r["LLR-ID"]
        parents = refs(r.get("SR-Refs"))
        if not parents:
            orphans.append(f"LLR {lid} has no SR parent")
        for p in parents:
            if p not in sr_ids:
                orphans.append(f"LLR {lid} references unknown {p}")
        if lid not in tc_refs:
            orphans.append(f"LLR {lid} has no test (TC)")

    valid = sr_ids | llr_ids
    for r in tcs:
        tid = r["TC-ID"]
        verified = refs(r.get("Verifies"))
        if not verified:
            orphans.append(f"TC {tid} verifies nothing")
        for x in verified:
            if x not in valid:
                orphans.append(f"TC {tid} references unknown {x}")

    for u in sorted(un_ids):
        if u not in sr_un_refs:
            orphans.append(f"UN {u} has no SR")

    status_findings = []
    if args.require_verified:
        for r in srs:
            if (
                r.get("Verification", "") == "Test"
                and r.get("Status", "") != "Verified"
            ):
                status_findings.append(
                    f"SR {r['SR-ID']} is Verification=Test but Status="
                    f"{r.get('Status', '') or '(blank)'} (G3 requires Verified)"
                )

    lines = (
        [
            "# Coverage & Traceability Report",
            "",
            "_Generated by `scripts/trace.py`. Do not edit by hand._",
            "",
            "| Metric | Count |",
            "|---|---|",
            f"| User needs (UN) | {len(un_ids)} |",
            f"| System requirements (SR) | {len(srs)} |",
            f"| Low-level requirements (LLR) | {len(llrs)} |",
            f"| Test cases (TC) | {len(tcs)} |",
            f"| Orphans | {len(orphans)} |",
        ]
        + (
            [f"| Status findings | {len(status_findings)} |"]
            if args.require_verified
            else []
        )
        + [
            "",
            "## SR -> LLR -> TC matrix",
            "",
            "| SR | LLRs | TCs | Status |",
            "|---|---|---|---|",
        ]
    )
    for r in srs:
        sid = r["SR-ID"]
        kids = " ".join(x["LLR-ID"] for x in llrs if sid in refs(x.get("SR-Refs")))
        tests = " ".join(x["TC-ID"] for x in tcs if sid in refs(x.get("Verifies")))
        lines.append(f"| {sid} | {kids} | {tests} | {r.get('Status', '')} |")
    lines += ["", "## Orphans", ""]
    lines += ["None. Full coverage."] if not orphans else [f"- {o}" for o in orphans]
    if args.require_verified:
        lines += ["", "## Status findings (--require-verified)", ""]
        lines += (
            ["None. Every Verification=Test SR is Verified."]
            if not status_findings
            else [f"- {s}" for s in status_findings]
        )

    out = docs / "test" / "report.md"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")

    print(
        f"Traceability: UN={len(un_ids)} SR={len(srs)} LLR={len(llrs)} "
        f"TC={len(tcs)} orphans={len(orphans)}"
        + (f" status-findings={len(status_findings)}" if args.require_verified else "")
        + f". Report -> {out}"
    )
    if args.strict and (orphans or status_findings):
        sys.exit(1)


if __name__ == "__main__":
    main()
