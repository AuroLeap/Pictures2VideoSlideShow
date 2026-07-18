#!/usr/bin/env python3
"""Design-time runtime-flow check: the G2 reviewer reads diagrams, not CSV rows.

The generated code map and `--flow` exist only once code exists (G3+). But the
behavior most often misread at G2 — concurrency, background work, what blocks
on what — is decided *with the LLRs*. So PROCESS.md §3 requires a hand-authored
**"Runtime flows"** section in the architecture doc at G2: Mermaid sequence
diagrams of the key runtime scenarios, each citing the SR/LLR ids it renders,
so a human can verify intended behavior by reading the flow instead of
inferring it from registry rows.

This checker keeps that section honest (stdlib only, like trace.py):

    python scripts/check_flows.py [--doc docs/architecture.md] [--docs docs]
                                  [--require N] [--no-placeholders]

Failures (exit 1):
    - the doc has no "Runtime flows" heading;
    - the section contains fewer than N (default 1) ```mermaid blocks;
    - a diagram cites no SR/LLR id at all (flows must stay traceable);
    - a cited SR/LLR/SN/TC id does not exist in the registries.

Placeholder ids ending in "-000" (the templates' examples) satisfy the
"cites an id" rule and are never validated, so a fresh scaffold starts green.
--no-placeholders (wire it in from G2 on) instead *flags* every cited "-000"
id, so a real authored flow can't keep citing the template's example ids.
"""

import argparse
import csv
import re
import sys
from pathlib import Path


def _utf8_console():
    """Emit UTF-8 to stdout/stderr whatever the OS console codepage is, so a
    non-ASCII heading echoed in a finding can't raise UnicodeEncodeError on a
    legacy Windows cp1252 console. Python 3.7+ streams expose `.reconfigure`;
    guard for the rest."""
    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding="utf-8")
        except (AttributeError, ValueError):
            pass


ID_RE = re.compile(r"\b(SR|LLR|SN|TC)-\d+\b")
HEADING_RE = re.compile(r"^(#{1,6})\s+(.*)$")
SECTION_TITLE = "runtime flows"


def load_ids(docs):
    """Collect the known ids per kind from the registries (trace.py's sources)."""

    def col(path, key):
        if not path.exists():
            return set()
        with path.open(newline="", encoding="utf-8-sig") as f:
            return {r[key] for r in csv.DictReader(f) if r.get(key)}

    known = {
        "SR": col(docs / "requirements" / "system-requirements.csv", "SR-ID"),
        "LLR": col(docs / "requirements" / "low-level-requirements.csv", "LLR-ID"),
        "TC": col(docs / "test" / "test-cases.csv", "TC-ID"),
        "SN": set(),
    }
    sn_md = docs / "requirements" / "stakeholder-needs.md"
    if sn_md.exists():
        known["SN"] = set(re.findall(r"\bSN-\d+\b", sn_md.read_text(encoding="utf-8")))
    return known


def flows_section(text):
    """Return the 'Runtime flows' section body, or None when the heading is
    absent. The section runs to the next heading of the same or higher level."""
    lines = text.splitlines()
    start = level = None
    for i, line in enumerate(lines):
        m = HEADING_RE.match(line)
        if m and m.group(2).strip().lower().startswith(SECTION_TITLE):
            start, level = i + 1, len(m.group(1))
            break
    if start is None:
        return None
    for j in range(start, len(lines)):
        m = HEADING_RE.match(lines[j])
        if m and len(m.group(1)) <= level:
            return "\n".join(lines[start:j])
    return "\n".join(lines[start:])


def mermaid_blocks(section):
    """The ```mermaid fenced blocks inside the section, in order."""
    return re.findall(r"```mermaid\s*\n(.*?)```", section, flags=re.DOTALL)


def main():
    _utf8_console()
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--doc",
        default="docs/architecture.md",
        help="doc holding the Runtime flows section (default: docs/architecture.md)",
    )
    ap.add_argument("--docs", default="docs", help="docs directory (default: docs)")
    ap.add_argument(
        "--require",
        type=int,
        default=1,
        metavar="N",
        help="minimum number of flow diagrams (default: 1)",
    )
    ap.add_argument(
        "--no-placeholders",
        action="store_true",
        help="flag cited '-000' template ids instead of ignoring them (G2 on)",
    )
    args = ap.parse_args()

    doc = Path(args.doc)
    if not doc.exists():
        print(f"check_flows: FAIL - {doc} does not exist")
        sys.exit(1)

    section = flows_section(doc.read_text(encoding="utf-8"))
    if section is None:
        print(
            f'check_flows: FAIL - no "Runtime flows" heading in {doc} '
            "(required at G2; see process.md §3 'Design-time runtime flows')"
        )
        sys.exit(1)

    blocks = mermaid_blocks(section)
    problems = []
    if len(blocks) < args.require:
        problems.append(
            f"section has {len(blocks)} mermaid diagram(s); {args.require} required"
        )

    known = load_ids(Path(args.docs))
    for n, block in enumerate(blocks, 1):
        kinds = {m.group(1) for m in ID_RE.finditer(block)}
        if not kinds & {"SR", "LLR"}:
            problems.append(
                f"diagram {n} cites no SR/LLR id - every flow must say which "
                "requirements it renders"
            )
    for m in ID_RE.finditer(section):
        rid, kind = m.group(0), m.group(1)
        if rid.endswith("-000"):
            if args.no_placeholders:
                problems.append(
                    f"placeholder id still cited: {rid} (replace the template "
                    "example flow with real SR/LLR ids before this gate)"
                )
            continue  # otherwise a template placeholder - never validated
        if rid not in known[kind]:
            problems.append(f"unknown id cited: {rid}")

    if problems:
        for p in sorted(set(problems)):
            print(f"check_flows: FAIL - {p}")
        sys.exit(1)
    cited_total = len({m.group(0) for m in ID_RE.finditer(section)})
    print(
        f"check_flows: OK - {len(blocks)} flow diagram(s), "
        f"{cited_total} requirement id(s) cited, all known."
    )


if __name__ == "__main__":
    main()
