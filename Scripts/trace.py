#!/usr/bin/env python3
"""Traceability join + orphan report for the SN->SR->LLR->TC registries.

Stack-agnostic reference implementation (Python 3, standard library only — no
pip installs). Drop it in a new repo as `scripts/trace.py` and wire it into the
check harness / CI. It is the generated "traceability matrix" referenced by
PROCESS.md: it never needs hand-maintaining.

Usage:
    python scripts/trace.py [--strict] [--strict-integrity] [--require-verified]
                            [--phase LIST] [--no-placeholders] [--strict-schema]
                            [--html] [--root DIR] [--docs DIR]

Reads (relative to --docs, default "<root>/docs"; --root defaults to "."):
    requirements/system-requirements.csv   (cols: SR-ID, SN-Refs, Verification, Status, ...)
    requirements/low-level-requirements.csv (cols: LLR-ID, SR-Refs, ...)
    test/test-cases.csv                     (cols: TC-ID, Verifies, ...)
    requirements/stakeholder-needs.md       (optional; SN-### ids scraped for SN->SR coverage)
    requirements/performance-budgets.csv    (optional; PB-### perf/resource budgets, §9 —
                                             each row's Refs must back-link a real SR/LLR/Module)
    requirements/modules.csv                 (optional; MOD-### coordinator module registry,
                                             MULTI_REPO.md — each row's DelegatedSRs must name a
                                             real coordinator SR; the multi-repo layer only)
    requirements/procurement.csv             (optional; PART-### purchased/external parts,
                                             process-options.md — each row's IF-Ref names the
                                             owning interface row of record (MULTI_REPO.md §3.3);
                                             integrity-checked only, like a MOD row with no
                                             DelegatedSRs)

Writes:
    test/report.md  — counts, the SR->LLR->TC matrix, the orphan list, and two
        rendered views of the same join: a line-reviewable SN->SR->LLR->TC text
        outline and a small, diff-friendly Mermaid `graph LR` DAG colored by
        orphan/draft state.
    test/report.html (only with --html) — a dependency-free, collapsible
        <details> tree of the full graph (inline CSS, zero JS) that scales to any
        size. A generated composite artifact: gitignored, never the review
        surface — review the registry CSVs (process.md §3 "Reviewability").

Exit code: 0 normally; with --strict, 1 if any orphan (or, with
--require-verified, any status finding) exists — use in gates.

Orphan rules (the method rules are stated once, in process.md §4):
    - SR with no LLR (unless Verification is Analysis/Inspection — those have no
      code to decompose; Demonstration/Manual SRs still describe behavior the
      software implements, so they keep the LLR requirement)
    - SR with no TC (every SR needs ≥1 TC row regardless of method; for human
      methods the TC records the procedure with Automated=No)
    - SR with no SN link (only when stakeholder-needs.md provides real SN ids —
      the G1 "every SR links ≥1 SN" criterion, machine-checked)
    - LLR with no SR parent, or referencing an unknown SR
    - LLR with no TC
    - TC that verifies nothing, or references an unknown SR/LLR
    - SN with no SR (only when stakeholder-needs.md is present)
--require-verified adds the G3 status criterion:
    - SR with Verification=Test whose Status is not Verified
--phase scopes that status criterion to a delivery phase (process.md §4
"Phased delivery"): SRs may carry an optional `Phase` column (e.g. v1, v2);
`--phase v1` (or a cumulative list, `--phase v1,v2`) exempts SRs tagged with
*other* phases from --require-verified and reports them as phase-deferred —
the exemption is explicit, never silent. A blank/absent Phase means the SR is
in scope for every phase. Orphan rules are phase-blind: every SR keeps its
LLR + TC rows regardless of phase.

Always (independent of --strict-schema), structural integrity is checked:
    - a duplicated SR/LLR/TC/PB/MOD id (the join would otherwise silently dedupe it)
    - a malformed id (not "PREFIX-<digits>")
    - a performance-budget row (PB-###, §9) whose Refs name an unknown
      SR/LLR/Module, or that back-links nothing — the budgets registry is off the
      spine but must stay traceable to it (the PB-000 example row is ignored, so
      the optional registry never blocks a gate a project doesn't use)
    - a coordinator module row (MOD-###, MULTI_REPO.md — the multi-repo layer)
      whose DelegatedSRs name an unknown coordinator SR; an external/reused part
      referenced only via the IF-### catalog may delegate nothing, so an empty
      back-link is allowed here (unlike PB). The MOD-000 example row is ignored,
      so the optional registry never blocks a single-repo project's gate
    - a purchased/external part row (PART-###, process-options.md) with a
      malformed/duplicate PART- id. Its IF-Ref names the owning interface row of
      record (MULTI_REPO.md §3.3) but is NOT resolved here: trace.py never reads
      interfaces.csv (the IF-### tier is off the joined spine), so PART is
      integrity-checked only — like a MOD row that delegates nothing. The
      PART-000 example row is ignored, so the optional registry never blocks a gate
These join `--strict`'s failure set like orphans do. `--strict-integrity` fails
on *only* this integrity class: it is the always-valid floor the pre-commit hook
runs on every commit — a duplicated or malformed id is wrong at any stage, while
orphans are a G2+ *gate* criterion (a mid-G1 registry legitimately has SRs with
no LLR/TC yet, and must still be committable).

--no-placeholders flags any leftover template example row (id ending "-000") as
a finding — wire it in from G2 on (a fresh scaffold is exempt only until you
claim a gate). Without it, "-000" example rows are ignored so a fresh scaffold
starts green.

--strict-schema adds data-quality checks over the real (non-placeholder) rows:
    - required fields are non-empty (SR: SR-ID, Title, SN-Refs, Requirement,
      AcceptanceCriteria, Priority, Verification, Status; LLR: LLR-ID, SR-Refs,
      Title, Module, CodeSymbol, Detail, Status; TC: TC-ID, Verifies, Level,
      Method, Tier, Expected, Automated, Status);
    - the two *closed* vocabularies the method defines (process.md §4) hold:
      SR Verification in {Test, Demonstration, Manual, Analysis, Inspection},
      TC Tier in {Smoke, Full, Release}. Priority/Status are deliberately NOT
      enumerated — the method leaves them open (e.g. Priority S, Status Planned).
"""

import argparse
import csv
import re
import sys
from pathlib import Path


def _utf8_console():
    """Emit UTF-8 to stdout/stderr whatever the OS console codepage is. Kit
    scripts print non-ASCII (an em-dash WARNING, `§` refs) that a legacy Windows
    cp1252 console raises UnicodeEncodeError on — wedging the run, not just
    mojibaking. Python 3.7+ streams expose `.reconfigure`; guard for the rest."""
    for s in (sys.stdout, sys.stderr):
        try:
            s.reconfigure(encoding="utf-8")
        except (AttributeError, ValueError):
            pass


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


# Id syntax per registry (for the always-on integrity check).
ID_PATTERNS = {
    "SR": re.compile(r"^SR-\d+$"),
    "LLR": re.compile(r"^LLR-\d+$"),
    "TC": re.compile(r"^TC-\d+$"),
    "PB": re.compile(r"^PB-\d+$"),  # optional performance-budgets registry (§9)
    "MOD": re.compile(
        r"^MOD-\d+$"
    ),  # optional coordinator module registry (MULTI_REPO.md)
    "PART": re.compile(
        r"^PART-\d+$"
    ),  # optional purchased/external parts registry (process-options.md)
}

# Fields that must be non-empty under --strict-schema. Deliberately omits the
# optional columns (Rationale, Permutations, Phase, TestRefs, Parameters).
REQUIRED_FIELDS = {
    "SR": [
        "SR-ID",
        "Title",
        "SN-Refs",
        "Requirement",
        "AcceptanceCriteria",
        "Priority",
        "Verification",
        "Status",
    ],
    "LLR": ["LLR-ID", "SR-Refs", "Title", "Module", "CodeSymbol", "Detail", "Status"],
    "TC": [
        "TC-ID",
        "Verifies",
        "Level",
        "Method",
        "Tier",
        "Expected",
        "Automated",
        "Status",
    ],
}

# The only *closed* vocabularies the method defines (process.md §4). Priority and
# Status are intentionally left open, so they are not validated here.
ENUM_FIELDS = {
    "SR": {
        "Verification": {"Test", "Demonstration", "Manual", "Analysis", "Inspection"}
    },
    "TC": {"Tier": {"Smoke", "Full", "Release"}},
}


def id_key(label):
    return label + "-ID"


def integrity_findings(label, raw_rows):
    """Duplicated or malformed ids in one registry (example '-000' rows skipped —
    those are the placeholder check's job, never an integrity error)."""
    key, pattern = id_key(label), ID_PATTERNS[label]
    found, seen = [], set()
    for r in raw_rows:
        rid = r.get(key)
        if not rid or is_example(rid):
            continue
        if not pattern.match(rid):
            found.append(f"{label} id {rid!r} is malformed (expected {label}-<digits>)")
        elif rid in seen:
            found.append(f"{label} id {rid} is duplicated")
        seen.add(rid)
    return found


def placeholder_findings(label, raw_rows):
    """Leftover template example rows (ids ending '-000') in one registry."""
    key = id_key(label)
    return [
        f"{label} placeholder row {r[key]} still present "
        "(replace the template example before this gate)"
        for r in raw_rows
        if r.get(key) and is_example(r[key])
    ]


def scan_sn_placeholders(sn_md):
    """Sorted unique '-000' SN ids still present in stakeholder-needs.md (if it exists)."""
    if not sn_md.exists():
        return []
    text = sn_md.read_text(encoding="utf-8")
    return sorted({u for u in re.findall(r"\bSN-\d+\b", text) if is_example(u)})


def schema_findings(label, rows):
    """Empty required fields and out-of-vocabulary Verification/Tier values, over
    the real (non-placeholder) rows of one registry."""
    key = id_key(label)
    out = []
    for r in rows:
        rid = r[key]
        for col in REQUIRED_FIELDS[label]:
            if not (r.get(col) or "").strip():
                out.append(f"{label} {rid} has empty required field {col}")
        for col, allowed in ENUM_FIELDS.get(label, {}).items():
            val = (r.get(col) or "").strip()
            if val and val not in allowed:
                out.append(
                    f"{label} {rid} has {col}={val!r} (allowed: "
                    f"{', '.join(sorted(allowed))})"
                )
    return out


# --- Generated traceability views --------------------------------------------
# The registries are the reviewed source of truth; everything below is a
# *rendering* of the same join, regenerated every run (process.md §3
# "Reviewability"). Three views because none is both line-reviewable and
# big-graph-scalable: the text outline reviews line-by-line and scales to any
# size; the Mermaid DAG is small and diff-friendly; the HTML tree browses the
# full graph at any size. All are stdlib string-building — no dependency.


def _cell(row, col):
    return (row.get(col) or "").strip()


def _node_class(rid, status, orphan_ids):
    """A node's view class: orphan (a trace finding) outranks draft (a status)."""
    if rid in orphan_ids:
        return "orphan"
    if status.lower() == "draft":
        return "draft"
    return ""


def _node(rid, status, title, orphan_ids, children=None):
    return {
        "id": rid,
        "status": status,
        "title": title,
        "cls": _node_class(rid, status, orphan_ids),
        "children": children or [],
    }


def _group(label, children):
    """A synthetic, unflagged parent for rows with no valid parent, so both tree
    views surface the same orphan tails the Orphans section lists."""
    return {"id": label, "status": "", "title": "", "cls": "", "children": children}


def build_forest(sn_ids, srs, llrs, tcs, orphan_ids):
    """The SN -> SR -> LLR -> TC chain as nested nodes, plus synthetic groups for
    rows with no valid parent. Shared by the text outline and the HTML tree."""

    def tc_node(t):
        return _node(t["TC-ID"], _cell(t, "Status"), _cell(t, "Method"), orphan_ids)

    def llr_node(lr):
        lid = lr["LLR-ID"]
        kids = [tc_node(t) for t in tcs if lid in refs(t.get("Verifies"))]
        return _node(lid, _cell(lr, "Status"), _cell(lr, "Title"), orphan_ids, kids)

    def sr_node(s):
        sid = s["SR-ID"]
        own_llrs = {lr["LLR-ID"] for lr in llrs if sid in refs(lr.get("SR-Refs"))}
        kids = [llr_node(lr) for lr in llrs if sid in refs(lr.get("SR-Refs"))]
        # TCs verifying the SR directly but none of its LLRs (so a TC that already
        # appears under an LLR of this SR is not also repeated under the SR).
        for t in tcs:
            verifies = set(refs(t.get("Verifies")))
            if sid in verifies and not verifies & own_llrs:
                kids.append(tc_node(t))
        return _node(sid, _cell(s, "Status"), _cell(s, "Title"), orphan_ids, kids)

    sr_ids = {s["SR-ID"] for s in srs}
    llr_ids = {lr["LLR-ID"] for lr in llrs}
    roots = []
    for sn in sorted(sn_ids):
        kids = [sr_node(s) for s in srs if sn in refs(s.get("SN-Refs"))]
        roots.append(_node(sn, "", "", orphan_ids, kids))
    rootless_srs = [s for s in srs if not sn_ids & set(refs(s.get("SN-Refs")))]
    if rootless_srs:
        label = (
            "(SRs with no linked stakeholder need)"
            if sn_ids
            else "(system requirements)"
        )
        roots.append(_group(label, [sr_node(s) for s in rootless_srs]))
    rootless_llrs = [lr for lr in llrs if not sr_ids & set(refs(lr.get("SR-Refs")))]
    if rootless_llrs:
        roots.append(
            _group("(LLRs with no SR parent)", [llr_node(lr) for lr in rootless_llrs])
        )
    valid = sr_ids | llr_ids
    rootless_tcs = [t for t in tcs if not valid & set(refs(t.get("Verifies")))]
    if rootless_tcs:
        roots.append(
            _group("(TCs verifying nothing valid)", [tc_node(t) for t in rootless_tcs])
        )
    return roots


def _flag_suffix(node):
    """The inline ` [Status] [orphan] — Title` tail shared by both tree views."""
    bits = []
    if node["status"]:
        bits.append("[{}]".format(node["status"]))
    if node["cls"] == "orphan":
        bits.append("[orphan]")
    suffix = (" " + " ".join(bits)) if bits else ""
    if node["title"]:
        suffix += " — " + node["title"]
    return suffix


def outline_lines(roots):
    """Indented Markdown list of the forest — pure text, so it reviews line-by-
    line and scales to any project size."""
    out = []

    def walk(node, depth):
        out.append("{}- {}{}".format("  " * depth, node["id"], _flag_suffix(node)))
        for child in node["children"]:
            walk(child, depth + 1)

    for r in roots:
        walk(r, 0)
    return out or ["_(no requirements yet)_"]


MERMAID_CLASSDEFS = [
    "    classDef orphan fill:#ffd6d6,stroke:#cc0000,color:#000;",
    "    classDef draft fill:#fff3cd,stroke:#cc9900,color:#000;",
]


def _mermaid_id(rid):
    # Mermaid node ids can't carry '-'/'.'-style separators — sanitize to '_'.
    return re.sub(r"\W", "_", rid)


def _mermaid_label(rid, title):
    if not title:
        return rid
    short = title if len(title) <= 40 else title[:39] + "…"
    return "{} — {}".format(rid, short).replace('"', "'")


def mermaid_graph(sn_ids, srs, llrs, tcs, orphan_ids):
    """A `graph LR` DAG of the chain (a TC verifies its SR *and* its LLR), colored
    by orphan/draft state via classDef. Kept small/diff-friendly on purpose — the
    HTML view is the one that scales."""
    sr_ids = {s["SR-ID"] for s in srs}
    llr_ids = {lr["LLR-ID"] for lr in llrs}
    nodes = {}  # rid -> (label, cls); dict insertion order keeps output stable
    edges = set()

    def add(rid, label, cls):
        nodes[rid] = (label, cls)

    for sn in sorted(sn_ids):
        add(sn, sn, "orphan" if sn in orphan_ids else "")
    for s in srs:
        sid = s["SR-ID"]
        add(sid, _mermaid_label(sid, _cell(s, "Title")),
            _node_class(sid, _cell(s, "Status"), orphan_ids))  # fmt: skip
        for u in refs(s.get("SN-Refs")):
            if u in sn_ids:
                edges.add((u, sid))
    for lr in llrs:
        lid = lr["LLR-ID"]
        add(lid, _mermaid_label(lid, _cell(lr, "Title")),
            _node_class(lid, _cell(lr, "Status"), orphan_ids))  # fmt: skip
        for p in refs(lr.get("SR-Refs")):
            if p in sr_ids:
                edges.add((p, lid))
    for t in tcs:
        tid = t["TC-ID"]
        add(tid, tid, _node_class(tid, _cell(t, "Status"), orphan_ids))
        for x in refs(t.get("Verifies")):
            if x in sr_ids or x in llr_ids:
                edges.add((x, tid))

    lines = ["```mermaid", "graph LR"] + MERMAID_CLASSDEFS
    if not nodes:
        lines.append("    empty[No requirements yet]")
    for rid, (label, _cls) in nodes.items():
        lines.append('    {}["{}"]'.format(_mermaid_id(rid), label))
    for a, b in sorted(edges):
        lines.append("    {} --> {}".format(_mermaid_id(a), _mermaid_id(b)))
    for rid, (_label, cls) in nodes.items():
        if cls:
            lines.append("    class {} {};".format(_mermaid_id(rid), cls))
    lines.append("```")
    return lines


def _esc(text):
    return (
        text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


HTML_HEAD = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Traceability map</title>
<style>
  body { font: 14px/1.5 system-ui, sans-serif; margin: 2rem; color: #222; }
  h1 { font-size: 1.3rem; }
  details { margin: 0.15rem 0 0.15rem 1.1rem; }
  summary { cursor: pointer; }
  .leaf { margin: 0.15rem 0 0.15rem 1.1rem; }
  .orphan { color: #b00020; font-weight: 600; }
  .draft { color: #8a6d00; }
  .note { color: #666; }
</style>
</head>
<body>
<h1>Traceability map</h1>
<p class="note">Generated by <code>scripts/trace.py --html</code>. Do not edit by
hand; review the registry CSVs, not this render (process.md §3 "Reviewability").</p>
"""

HTML_TAIL = "</body>\n</html>\n"


def html_document(roots):
    """A dependency-free, collapsible <details> tree of the full graph — inline
    CSS, zero JS, self-contained — for browse/onboard/audit at any size."""

    def walk(node, depth):
        pad = "  " * depth
        label = _esc(node["id"]) + _esc(_flag_suffix(node))
        if node["children"]:
            cls = ' class="{}"'.format(node["cls"]) if node["cls"] else ""
            out = ["{}<details open><summary{}>{}</summary>".format(pad, cls, label)]
            for child in node["children"]:
                out += walk(child, depth + 1)
            out.append("{}</details>".format(pad))
            return out
        leaf_cls = ("leaf " + node["cls"]).strip()
        return ['{}<div class="{}">{}</div>'.format(pad, leaf_cls, label)]

    body = []
    for r in roots:
        body += walk(r, 0)
    if not body:
        body = ['<p class="note">No requirements yet.</p>']
    return HTML_HEAD + "\n".join(body) + "\n" + HTML_TAIL


def main():
    _utf8_console()
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--strict", action="store_true", help="exit 1 if any orphan / status finding"
    )
    ap.add_argument(
        "--strict-integrity",
        action="store_true",
        help="exit 1 only on integrity findings (duplicate/malformed ids) — the "
        "always-valid floor the pre-commit hook runs; orphans stay gate-scoped",
    )
    ap.add_argument(
        "--require-verified",
        action="store_true",
        help="G3 criterion: flag Verification=Test SRs not Status=Verified",
    )
    ap.add_argument(
        "--phase",
        default=None,
        help="comma-separated phases in scope (e.g. v1 or v1,v2): scopes "
        "--require-verified to SRs whose Phase is blank or listed",
    )
    ap.add_argument(
        "--no-placeholders",
        action="store_true",
        help="flag any leftover '-000' template example row (use from G2 on)",
    )
    ap.add_argument(
        "--strict-schema",
        action="store_true",
        help="also require non-empty required fields and valid "
        "Verification/Tier values on the real rows",
    )
    ap.add_argument(
        "--html",
        action="store_true",
        help="also write test/report.html — a dependency-free collapsible tree "
        "of the full graph (gitignored composite artifact)",
    )
    # --root/--docs are the uniform path flags across trace.py, check_docs.py,
    # and check_perf.py: --docs is the docs dir; --root (default ".") is its
    # parent, so a repo whose docs live elsewhere passes one --root. An explicit
    # --docs wins; otherwise it is <root>/docs.
    ap.add_argument("--root", default=".", help="repo root (default: .)")
    ap.add_argument(
        "--docs",
        default=None,
        help="docs directory (default: <root>/docs)",
    )
    args = ap.parse_args()
    docs = Path(args.docs) if args.docs else Path(args.root) / "docs"

    raw_srs = load_csv(docs / "requirements" / "system-requirements.csv")
    raw_llrs = load_csv(docs / "requirements" / "low-level-requirements.csv")
    raw_tcs = load_csv(docs / "test" / "test-cases.csv")
    # Optional, off-spine coordination registry (process.md §9); absent file -> [].
    raw_pbs = load_csv(docs / "requirements" / "performance-budgets.csv")
    # Optional coordinator module registry (MULTI_REPO.md, the multi-repo layer);
    # absent file -> [].
    raw_mods = load_csv(docs / "requirements" / "modules.csv")
    # Optional purchased/external parts registry (process-options.md); absent
    # file -> []. Integrity-checked only (IF-Ref points at the off-spine IF-###
    # tier, which trace.py does not read).
    raw_parts = load_csv(docs / "requirements" / "procurement.csv")

    # The working sets exclude template example rows (ids ending "-000") so a
    # fresh scaffold has nothing to orphan; the raw lists above keep them for the
    # placeholder and integrity checks below.
    srs = [r for r in raw_srs if r.get("SR-ID") and not is_example(r["SR-ID"])]
    llrs = [r for r in raw_llrs if r.get("LLR-ID") and not is_example(r["LLR-ID"])]
    tcs = [r for r in raw_tcs if r.get("TC-ID") and not is_example(r["TC-ID"])]
    pbs = [r for r in raw_pbs if r.get("PB-ID") and not is_example(r["PB-ID"])]
    mods = [r for r in raw_mods if r.get("MOD-ID") and not is_example(r["MOD-ID"])]
    parts = [r for r in raw_parts if r.get("PART-ID") and not is_example(r["PART-ID"])]

    sn_ids = set()
    sn_md = docs / "requirements" / "stakeholder-needs.md"
    if sn_md.exists():
        sn_ids = {
            u
            for u in re.findall(r"\bSN-\d+\b", sn_md.read_text(encoding="utf-8"))
            if not is_example(u)
        }

    sr_ids = {r["SR-ID"] for r in srs}
    llr_ids = {r["LLR-ID"] for r in llrs}
    llr_sr_refs = {x for r in llrs for x in refs(r.get("SR-Refs"))}
    tc_refs = {x for r in tcs for x in refs(r.get("Verifies"))}
    sr_sn_refs = {x for r in srs for x in refs(r.get("SN-Refs"))}

    # orphan_ids collects the at-fault id for each finding, so the rendered views
    # below (outline/graph/HTML) can flag the same nodes the text list reports.
    orphans = []
    orphan_ids = set()
    for r in srs:
        sid = r["SR-ID"]
        analytic = r.get("Verification", "") in ("Analysis", "Inspection")
        if not analytic and sid not in llr_sr_refs:
            orphans.append(
                f"SR {sid} has no LLR (and Verification != Analysis/Inspection)"
            )
            orphan_ids.add(sid)
        if sid not in tc_refs:
            orphans.append(f"SR {sid} has no test (TC)")
            orphan_ids.add(sid)
        sn_parents = refs(r.get("SN-Refs"))
        # G1's "every SR links >=1 SN", machine-checked — but only when the SN
        # registry actually provides real ids (a project without a needs file,
        # or one holding only -000 placeholders, has no SN tier to link yet).
        if sn_ids and not sn_parents:
            orphans.append(f"SR {sid} links no SN (every SR needs >=1 SN-Ref)")
            orphan_ids.add(sid)
        for u in sn_parents:
            if sn_ids and u not in sn_ids:
                orphans.append(f"SR {sid} references unknown {u}")
                orphan_ids.add(sid)

    for r in llrs:
        lid = r["LLR-ID"]
        parents = refs(r.get("SR-Refs"))
        if not parents:
            orphans.append(f"LLR {lid} has no SR parent")
            orphan_ids.add(lid)
        for p in parents:
            if p not in sr_ids:
                orphans.append(f"LLR {lid} references unknown {p}")
                orphan_ids.add(lid)
        if lid not in tc_refs:
            orphans.append(f"LLR {lid} has no test (TC)")
            orphan_ids.add(lid)

    valid = sr_ids | llr_ids
    for r in tcs:
        tid = r["TC-ID"]
        verified = refs(r.get("Verifies"))
        if not verified:
            orphans.append(f"TC {tid} verifies nothing")
            orphan_ids.add(tid)
        for x in verified:
            if x not in valid:
                orphans.append(f"TC {tid} references unknown {x}")
                orphan_ids.add(tid)

    for u in sorted(sn_ids):
        if u not in sr_sn_refs:
            orphans.append(f"SN {u} has no SR")
            orphan_ids.add(u)

    # Performance budgets (process.md §9) sit off the spine but stay traceable:
    # each row's Refs must resolve to a real SR/LLR id or an LLR Module path.
    module_ids = {(lr.get("Module") or "").strip() for lr in llrs}
    module_ids.discard("")
    budget_targets = sr_ids | llr_ids | module_ids
    budget_findings = []
    for r in pbs:
        pid = r["PB-ID"]
        targets = refs(r.get("Refs"))
        if not targets:
            budget_findings.append(f"PB {pid} back-links nothing (Refs is empty)")
        for x in targets:
            if x not in budget_targets:
                budget_findings.append(f"PB {pid} references unknown {x}")

    # Coordinator module registry (MULTI_REPO.md, the multi-repo layer) sits off the
    # spine like PB, but its DelegatedSRs stay traceable *within* the coordinator
    # repo: each must name a real coordinator SR (delegation is at the SR tier,
    # §3.1). The cross-boundary link (a module SN's ParentRef back to this SR) points
    # into another repo, so no single trace.py run validates it — that reconciliation
    # is the deferred cross-repo join. An external/reused part referenced only via the
    # IF-### catalog may delegate nothing, so an empty back-link is allowed here.
    module_findings = []
    for r in mods:
        mid = r["MOD-ID"]
        for x in refs(r.get("DelegatedSRs")):
            if x not in sr_ids:
                module_findings.append(f"MOD {mid} delegates unknown {x}")

    phases = set(refs(args.phase)) if args.phase else None

    def in_phase(r):
        """Blank Phase = every phase; otherwise the SR's phase must be listed."""
        tag = (r.get("Phase") or "").strip()
        return phases is None or not tag or tag in phases

    status_findings = []
    phase_deferred = []
    if args.require_verified:
        for r in srs:
            if r.get("Verification", "") != "Test":
                continue
            if not in_phase(r):
                phase_deferred.append(
                    f"SR {r['SR-ID']} (Phase={r.get('Phase', '').strip()}) — "
                    "status check deferred to its own phase"
                )
                continue
            if r.get("Status", "") != "Verified":
                status_findings.append(
                    f"SR {r['SR-ID']} is Verification=Test but Status="
                    f"{r.get('Status', '') or '(blank)'} (G3 requires Verified)"
                )

    raw = {"SR": raw_srs, "LLR": raw_llrs, "TC": raw_tcs}
    real = {"SR": srs, "LLR": llrs, "TC": tcs}
    integrity = [f for label in raw for f in integrity_findings(label, raw[label])]
    # PB ids are integrity-checked too, but PB is kept out of the placeholder/
    # schema sweeps above: the budgets registry is optional (like interfaces.csv),
    # so a leftover PB-000 must never block a gate the project doesn't use.
    integrity += integrity_findings("PB", raw_pbs)
    # The coordinator module registry (MOD-###, MULTI_REPO.md) is the same kind of
    # optional off-spine registry — integrity-checked, but out of the placeholder/
    # schema sweeps, so a single-repo project's MOD-000 placeholder never blocks it.
    integrity += integrity_findings("MOD", raw_mods)
    # The purchased/external parts registry (PART-###, process-options.md) is the
    # same kind of optional off-spine registry — integrity-checked (malformed/
    # duplicate id), but out of the placeholder/schema sweeps and with no back-link
    # resolution (its IF-Ref points at the IF-### tier trace.py doesn't read), so a
    # project that buys nothing keeps its PART-000 placeholder without blocking a gate.
    integrity += integrity_findings("PART", raw_parts)
    placeholders = (
        [f for label in raw for f in placeholder_findings(label, raw[label])]
        + [f"SN placeholder {u} still present" for u in scan_sn_placeholders(sn_md)]
        if args.no_placeholders
        else []
    )
    schema = (
        [f for label in real for f in schema_findings(label, real[label])]
        if args.strict_schema
        else []
    )

    lines = (
        [
            "# Coverage & Traceability Report",
            "",
            "_Generated by `scripts/trace.py`. Do not edit by hand._",
            "",
            "| Metric | Count |",
            "|---|---|",
            f"| Stakeholder needs (SN) | {len(sn_ids)} |",
            f"| System requirements (SR) | {len(srs)} |",
            f"| Low-level requirements (LLR) | {len(llrs)} |",
            f"| Test cases (TC) | {len(tcs)} |",
            f"| Orphans | {len(orphans)} |",
            f"| Integrity findings | {len(integrity)} |",
        ]
        + (
            [f"| Status findings | {len(status_findings)} |"]
            if args.require_verified
            else []
        )
        + (
            [f"| Placeholder findings | {len(placeholders)} |"]
            if args.no_placeholders
            else []
        )
        + ([f"| Schema findings | {len(schema)} |"] if args.strict_schema else [])
        + (
            [
                f"| Performance budgets (PB) | {len(pbs)} |",
                f"| Budget findings | {len(budget_findings)} |",
            ]
            if pbs
            else []
        )
        + (
            [
                f"| Modules (MOD) | {len(mods)} |",
                f"| Module findings | {len(module_findings)} |",
            ]
            if mods
            else []
        )
        + ([f"| Purchased parts (PART) | {len(parts)} |"] if parts else [])
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

    forest = build_forest(sn_ids, srs, llrs, tcs, orphan_ids)
    lines += [
        "",
        "## Traceability outline",
        "",
        "_`SN -> SR -> LLR -> TC`; `[Status]` and `[orphan]` flags are inline._",
        "",
    ]
    lines += outline_lines(forest)
    lines += [
        "",
        "## Traceability graph",
        "",
        "_The chain as a DAG, colored by state (orphan/draft stand out). Small and "
        "diff-friendly; run `--html` for the scalable full-graph view._",
        "",
    ]
    lines += mermaid_graph(sn_ids, srs, llrs, tcs, orphan_ids)

    lines += ["", "## Orphans", ""]
    lines += ["None. Full coverage."] if not orphans else [f"- {o}" for o in orphans]
    lines += ["", "## Integrity", ""]
    lines += (
        ["None. Ids are unique and well-formed."]
        if not integrity
        else [f"- {f}" for f in integrity]
    )
    if pbs:
        lines += ["", "## Performance budgets (§9 back-links)", ""]
        lines += (
            [f"{len(pbs)} budget row(s); every Refs resolves to a real SR/LLR/Module."]
            if not budget_findings
            else [f"- {f}" for f in budget_findings]
        )
    if mods:
        lines += ["", "## Modules (MULTI_REPO.md delegation back-links)", ""]
        lines += (
            [f"{len(mods)} module row(s); every DelegatedSRs resolves to a real SR."]
            if not module_findings
            else [f"- {f}" for f in module_findings]
        )
    if parts:
        lines += ["", "## Purchased parts (process-options.md)", ""]
        lines += [
            f"{len(parts)} part row(s); each IF-Ref names its owning interface row "
            "of record (MULTI_REPO.md §3.3), integrity-checked only."
        ]
    if args.no_placeholders:
        lines += ["", "## Placeholders (--no-placeholders)", ""]
        lines += (
            ["None. No '-000' template rows remain."]
            if not placeholders
            else [f"- {f}" for f in placeholders]
        )
    if args.strict_schema:
        lines += ["", "## Schema findings (--strict-schema)", ""]
        lines += (
            ["None. Required fields present; Verification/Tier in vocabulary."]
            if not schema
            else [f"- {f}" for f in schema]
        )
    if args.require_verified:
        scope = f" — phase scope: {args.phase}" if phases else ""
        lines += ["", f"## Status findings (--require-verified{scope})", ""]
        lines += (
            ["None. Every in-scope Verification=Test SR is Verified."]
            if not status_findings
            else [f"- {s}" for s in status_findings]
        )
        if phase_deferred:
            lines += ["", "### Phase-deferred (explicitly out of scope)", ""]
            lines += [f"- {s}" for s in phase_deferred]

    out = docs / "test" / "report.md"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")

    html_out = None
    if args.html:
        html_out = docs / "test" / "report.html"
        html_out.write_text(html_document(forest), encoding="utf-8")

    print(
        f"Traceability: SN={len(sn_ids)} SR={len(srs)} LLR={len(llrs)} "
        f"TC={len(tcs)} orphans={len(orphans)} integrity={len(integrity)}"
        + (f" status-findings={len(status_findings)}" if args.require_verified else "")
        + (f" placeholders={len(placeholders)}" if args.no_placeholders else "")
        + (f" schema-findings={len(schema)}" if args.strict_schema else "")
        + (f" phase-deferred={len(phase_deferred)}" if phases else "")
        + (f" budgets={len(pbs)} budget-findings={len(budget_findings)}" if pbs else "")
        + (
            f" modules={len(mods)} module-findings={len(module_findings)}"
            if mods
            else ""
        )
        + (f" parts={len(parts)}" if parts else "")
        + f". Report -> {out}"
        + (f" + {html_out}" if html_out else "")
    )
    if args.strict and (
        orphans
        or status_findings
        or integrity
        or placeholders
        or schema
        or budget_findings
        or module_findings
    ):
        sys.exit(1)
    if args.strict_integrity and integrity:
        sys.exit(1)


if __name__ == "__main__":
    main()
