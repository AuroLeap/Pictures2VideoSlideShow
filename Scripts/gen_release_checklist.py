#!/usr/bin/env python3
"""Generate the human release checklist from the registries.

Stack-agnostic kit, stdlib-only (Python 3.8+). Most of the harness is machine-
checkable, but a release still needs a human to *exercise the real product* —
the Demonstration / Manual / Inspection items that no automated test can honestly
cover (G-Final in process.md). This script collects exactly those, back-linked to
their requirement ids, into a tick-box checklist so the release sign-off is
concrete and traceable instead of a vibe.

It pulls, from `docs/`:
    - Stakeholder needs (SN) + their acceptance intent -> "Does the product meet the need?"
    - System requirements whose Verification is Demonstration / Manual / Inspection
    - Release-tier test cases, and any non-automated (manual) test cases
    - Provided cross-project interfaces (IF, if present) -> contract still honored?
    - Performance budgets (PB, if present) -> still within allocation? (§9; the
      warn-tier runtime budgets never fail the gate, so a human confirms them here)

Each line is `- [ ] <ID> — <what to confirm> (refs)`. The output is a *generated
record*: regenerate it per release and keep the ticked copy as the sign-off
artifact (use --version to file it under docs/releases/).

Usage:
    python scripts/gen_release_checklist.py [--docs docs] [--version X]
                                            [--phase LIST] [--out PATH]

    --version  Stamp the checklist and write to docs/releases/checklist-<X>.md.
    --phase    Phased delivery (process.md §4): include only SRs whose Phase is
               blank or listed (e.g. v1 or v1,v2), and only the release-tier /
               manual TCs that verify an in-scope SR (or an LLR under one).
    --out      Explicit output path (overrides the default/--version location).
    default    Writes docs/release-checklist.md.
"""

import argparse
import csv
import datetime
import re
from pathlib import Path

HUMAN_METHODS = {"Demonstration", "Manual", "Inspection"}


def load_csv(path):
    if not path.exists():
        return []
    with path.open(newline="", encoding="utf-8-sig") as f:
        return list(csv.DictReader(f))


def is_example(rid):
    return (rid or "").endswith("-000")


def read_stakeholder_needs(md_path):
    """Parse the SN core-needs markdown table -> list of (SN-ID, need, acceptance)."""
    if not md_path.exists():
        return []
    rows = []
    header = None
    need_i = acc_i = None
    for line in md_path.read_text(encoding="utf-8").splitlines():
        if not line.strip().startswith("|"):
            header = None
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if header is None:
            if any("SN-ID" in c for c in cells):
                header = cells
                need_i = next((i for i, c in enumerate(header) if "Need" in c), 1)
                acc_i = next(
                    (i for i, c in enumerate(header) if "Acceptance" in c), None
                )
            continue
        if cells and re.match(r"SN-\d+$", cells[0]) and not is_example(cells[0]):
            need = cells[need_i] if need_i is not None and need_i < len(cells) else ""
            acc = cells[acc_i] if acc_i is not None and acc_i < len(cells) else ""
            rows.append((cells[0], need, acc))
    return rows


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--docs", default="docs")
    ap.add_argument("--version", default=None)
    ap.add_argument(
        "--phase",
        default=None,
        help="comma-separated phases in scope (blank Phase = every phase)",
    )
    ap.add_argument("--out", default=None)
    args = ap.parse_args()
    docs = Path(args.docs)

    needs = read_stakeholder_needs(docs / "requirements" / "stakeholder-needs.md")
    srs = [
        r
        for r in load_csv(docs / "requirements" / "system-requirements.csv")
        if r.get("SR-ID") and not is_example(r["SR-ID"])
    ]
    tcs = [
        r
        for r in load_csv(docs / "test" / "test-cases.csv")
        if r.get("TC-ID") and not is_example(r["TC-ID"])
    ]
    ifs = [
        r
        for r in load_csv(docs / "requirements" / "interfaces.csv")
        if r.get("IF-ID") and not is_example(r["IF-ID"])
    ]
    # Performance budgets (process.md §9): the warn-tier runtime budgets never
    # fail the gate, so the release checklist is where a human confirms them.
    pbs = [
        r
        for r in load_csv(docs / "requirements" / "performance-budgets.csv")
        if r.get("PB-ID") and not is_example(r["PB-ID"])
    ]

    phases = (
        {p for p in re.split(r"[;,\s]+", args.phase.strip()) if p}
        if args.phase
        else None
    )

    def in_phase(sr_row):
        tag = (sr_row.get("Phase") or "").strip()
        return phases is None or not tag or tag in phases

    in_scope_sr_ids = {r["SR-ID"] for r in srs if in_phase(r)}
    # An LLR is in scope when any of its parent SRs is, so TC `Verifies` cells
    # that cite only LLR ids still resolve to the right phase.
    llrs = [
        r
        for r in load_csv(docs / "requirements" / "low-level-requirements.csv")
        if r.get("LLR-ID") and not is_example(r["LLR-ID"])
    ]
    in_scope_ids = set(in_scope_sr_ids)
    for r in llrs:
        parents = [p for p in re.split(r"[;,\s]+", r.get("SR-Refs", "")) if p]
        if any(p in in_scope_sr_ids for p in parents):
            in_scope_ids.add(r["LLR-ID"])

    def tc_in_scope(tc_row):
        cited = [x for x in re.split(r"[;,\s]+", tc_row.get("Verifies", "")) if x]
        return phases is None or any(x in in_scope_ids for x in cited)

    human_srs = [
        r for r in srs if r.get("Verification", "") in HUMAN_METHODS and in_phase(r)
    ]
    # A blank Automated cell intentionally counts as manual: an unclassified test
    # must show up on the human checklist rather than silently drop off it.
    manual_tcs = [
        r
        for r in tcs
        if (
            r.get("Tier", "") == "Release"
            or (r.get("Automated", "").strip().lower() in ("no", "false", ""))
        )
        and tc_in_scope(r)
    ]
    provided_ifs = [r for r in ifs if r.get("Direction", "") == "Provides"]

    stamp = args.version or "(unreleased)"
    if phases:
        stamp += " — phase {}".format(args.phase)
    today = datetime.date.today().isoformat()
    L = [
        "# Release Checklist — {}".format(stamp),
        "",
        "_Generated by `scripts/gen_release_checklist.py` on {}. Tick each box "
        "after exercising the real product; keep the completed copy as the "
        "G-Release sign-off record._".format(today),
        "",
        "- Version / build under test: __________   Date: __________   "
        "Signed-off by: __________",
        "",
    ]

    L += ["## 1. Stakeholder needs met (acceptance)", ""]
    if needs:
        for uid, need, acc in needs:
            detail = acc or need or "confirm the need is met"
            L.append("- [ ] **{}** — {} ({})".format(uid, detail, uid))
    else:
        L.append("- [ ] _(no stakeholder needs registered)_")

    L += [
        "",
        "## 2. Human-verified requirements (Demonstration / Manual / Inspection)",
        "",
    ]
    if human_srs:
        for r in human_srs:
            L.append(
                "- [ ] **{}** [{}] — {} (AcceptanceCriteria of {})".format(
                    r["SR-ID"],
                    r.get("Verification", ""),
                    r.get("Title", "").strip(),
                    r["SR-ID"],
                )
            )
    else:
        L.append("- [ ] _(every requirement is automated — nothing manual to verify)_")

    L += ["", "## 3. Release-tier & manual test cases", ""]
    if manual_tcs:
        for r in manual_tcs:
            L.append(
                "- [ ] **{}** [{}] — {} (verifies {})".format(
                    r["TC-ID"],
                    r.get("Tier", "") or "Manual",
                    r.get("Method", "").strip(),
                    r.get("Verifies", ""),
                )
            )
    else:
        L.append("- [ ] _(no release-tier or manual test cases)_")

    if provided_ifs:
        L += ["", "## 4. Cross-project contracts still honored", ""]
        for r in provided_ifs:
            L.append(
                "- [ ] **{}** ({} {}) — {} still satisfies the published "
                "contract (refs {})".format(
                    r["IF-ID"],
                    r.get("Version", ""),
                    r.get("Stability", ""),
                    r.get("Counterpart", ""),
                    r.get("SR-Refs", ""),
                )
            )

    if pbs:
        L += ["", "## 5. Performance budgets within allocation (§9)", ""]
        for r in pbs:
            arrow = "≤" if (r.get("Direction") or "").strip() == "lower-better" else "≥"
            L.append(
                "- [ ] **{}** — {} {} {}{} ({}; refs {})".format(
                    r["PB-ID"],
                    r.get("Metric", "").strip(),
                    arrow,
                    r.get("Budget", "").strip(),
                    r.get("Unit", "").strip(),
                    r.get("Gate", "").strip() or "warn",
                    r.get("Refs", "").strip(),
                )
            )

    L += [
        "",
        "## 6. Release hygiene",
        "",
        "- [ ] `python scripts/check.py --gate G3 --tier release` is green "
        "(paste the output in the audit log).",
        "- [ ] CHANGELOG / release notes updated.",
        "- [ ] Version bumped; any changed `Stable` interface versions "
        "communicated to counterparts.",
        "- [ ] Docs (README / quick-reference) match the shipped behavior.",
        "",
    ]

    if args.out:
        out = Path(args.out)
    elif args.version:
        out = docs / "releases" / "checklist-{}.md".format(args.version)
    else:
        out = docs / "release-checklist.md"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(L) + "\n", encoding="utf-8")

    print(
        "Release checklist -> {}  (SN={} human-SR={} manual-TC={} IF={} PB={})".format(
            out, len(needs), len(human_srs), len(manual_tcs), len(provided_ifs), len(pbs)
        )
    )


if __name__ == "__main__":
    main()
