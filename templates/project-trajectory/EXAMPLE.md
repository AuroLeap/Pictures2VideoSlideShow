# Worked Example — one full UN → SR → LLR → TC chain

A concrete pattern to copy. The feature: **"export my records to a CSV I can
open in a spreadsheet"**, plus one **edge case** (interruption safety). It shows
the whole spine, including the two habits that keep projects maintainable:
**separate a pure testable core from the I/O shell**, and **capture an edge case
as its own requirement**.

---

## 1. User Need — `requirements/user-needs.md`

| UN-ID | Need (plain language) | Why it matters | Priority | Acceptance intent |
|---|---|---|---|---|
| UN-001 | Export my records to a file I can open in a spreadsheet. | The data is useless if I can't get it into Excel/Sheets. | M | A new user clicks/exports and the file opens in a spreadsheet with the right columns and all their rows. |

Edge-case table:

| UN-ID | Scenario | Expected behavior |
|---|---|---|
| UN-013 | Export interrupted (crash / power loss / cancel) mid-write | I never end up with a half-written file that looks complete; I can just run it again. |

## 2. System Requirements — `requirements/system-requirements.csv`

```csv
SR-ID,Title,UN-Refs,Requirement,Rationale,AcceptanceCriteria,Permutations,Priority,Verification,Status
SR-001,CSV export (RFC-4180),UN-001,"The system shall export records as RFC-4180 CSV with a header row.","Realizes UN-001 so the file opens cleanly in any spreadsheet.","Output parses as CSV; row count == records + 1 (header); columns match the documented schema in order; fields containing comma/quote/newline are quoted per RFC-4180.","field=set{plain,comma,quote,newline}",M,Test,Verified
SR-002,Atomic export write,UN-013,"The system shall write the export to a temporary file and atomically rename it to the final name only after a successful write.","Realizes UN-013 so an interrupted run never leaves a complete-looking partial file.","A run interrupted before completion leaves no file at the final path (only a distinguishable temp); re-running completes normally.","interrupt=set{during-write,before-rename}",M,Demonstration,Implemented
```

Note: each SR has **measurable** acceptance criteria a test can assert (not "exports correctly"), links its UN, and uses `Permutations` so one row covers many cases.

## 3. Low-Level Requirements — `requirements/low-level-requirements.csv`

```csv
LLR-ID,SR-Refs,Title,Module,CodeSymbol,Detail,TestRefs,Status
LLR-001,SR-001,Pure records->CSV serializer,src/export/csv,to_csv,"Pure function: records -> String. Header from the schema; values quoted per RFC-4180. No I/O — unit-testable in isolation.",(see TC),Implemented
LLR-002,SR-002,Atomic file write,src/export/io,write_atomic,"Write bytes to <path>.tmp, then rename to <path>; remove the tmp on any error. Rename is atomic on the same volume. The I/O shell around the pure core.",(see TC),Implemented
```

Note the split: **`to_csv` is a pure core** (cheap, exhaustive unit tests);
**`write_atomic` is the I/O shell** (a smaller number of integration tests).
Detail *decomposes* the SR — it doesn't restate it.

## 4. Test Cases — `test/test-cases.csv`

```csv
TC-ID,Verifies,Level,Method,Tier,Parameters,Expected,Automated,Status
TC-001,SR-001;LLR-001,Unit,"to_csv over records incl. special-character fields; parse the result back",Smoke,"field=set{plain,comma,quote,newline}","Satisfies SR-001 AcceptanceCriteria",Yes,Verified
TC-002,SR-002;LLR-002,Integration,"Abort write_atomic mid-write; assert no file at the final path and the tmp is cleaned; then a normal run succeeds",Full,"interrupt=set{during-write,before-rename}","Satisfies SR-002 AcceptanceCriteria",Yes,Verified
```

The `Tier` column controls when a test runs: the cheap `to_csv` unit test is
`Smoke` (every iteration); the slower interruption integration test is `Full`
(pre-merge). A test needing real hardware or a long soak would be `Release`.
Mark `Smoke` tests `@pytest.mark.smoke` and `Release` tests
`@pytest.mark.release`; an unmarked test lands in `Full` — the pre-merge tier —
by default, so `check.py --tier` can never silently skip it.

Each TC lists the SR **and** the LLR it covers (so both levels are covered by
one row), expands the requirement's `Permutations`, and **cites** the acceptance
criteria by id rather than paraphrasing them.

## 4b. Dimensional coverage — boundaries × combinations

`SR-001` has three variable inputs, so one happy-path test is not enough. Declare
the dimensions in its `Permutations` cell (note **boundary** values and a
strategy):

```
field=set{plain,comma,quote,newline}; size=range[0..2GiB]; enc=set{utf8,utf16}; @pairwise
```

`field` is an equivalence partition (one representative per special-char class);
`size` contributes its **boundaries** (empty `0` and the max `2GiB` — the
classic empty-input / overflow catchers); `enc` is two classes. The full product
is 4 × 2 × 2 = 16. Feed the cell to the generator:

```
$ python scripts/gen_cases.py --spec "field=set{plain,comma,quote,newline}; size=range[0..2GiB]; enc=set{utf8,utf16}; @pairwise" --id SR-001
# Dimensional analysis for SR-001
  - field (4 values): plain, comma, quote, newline
  - size (2 values): *0*, *2GiB*   (* = boundary)
  - enc (2 values): utf8, utf16
  strategy: pairwise   cases: 8  (full product = 16; 50% reduction)
```

Eight cases instead of sixteen, yet every value of every dimension is still paired
with every value of the others — e.g. `quote` is tried at both `0` and `2GiB` and
in both encodings. TC-001 then **expands** these into its `Parameters` (or a
parametrized test) instead of sixteen near-duplicate rows. If this were an
expensive integration path, `--strategy boundaries` would drop it to a handful of
extreme-corner cases for the `Release` tier; if it were a corruption-risk path,
`@full` would keep all sixteen. **Match the strategy to risk and run cost; let the
generator produce the combinations.**

## 5. Code back-links (in the source)

```rust
/// Serialize records to RFC-4180 CSV (header + one row each).
// Implements: SR-001, LLR-001
pub fn to_csv(schema: &Schema, records: &[Record]) -> String { /* ... */ }

#[test]
fn to_csv_quotes_special_fields_sr001() { /* ...asserts SR-001 AC... */ }
```

The test name embeds the verified id, and the item is annotated `Implements:`.
The CSV columns are authoritative; these annotations keep code and registries
honest and greppable.

## 6. The traceability result

Running `python scripts/trace.py --strict` over this chain reports:

```
Traceability: UN=2 SR=2 LLR=2 TC=2 orphans=0. Report -> docs/test/report.md
```

**Zero orphans**: every UN has an SR, every SR has an LLR + a TC, every LLR has a
parent + a TC, every TC verifies a known id. That is the bar each gate enforces.

---

### What to copy from this pattern

- One **measurable** SR per need; push value-sets into `Permutations`, not
  duplicate rows.
- **Pure core vs. I/O/GUI shell** — it's the single biggest lever for testability
  and for keeping logic deduplicated and readable.
- **Edge cases are first-class requirements** (UN-013 → SR-002), not afterthoughts.
- Tests **cite** acceptance criteria by id; code **annotates** the ids; the matrix
  is **generated**, never hand-kept.
