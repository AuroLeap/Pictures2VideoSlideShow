# Architecture (one page)

Owned by the **Software Engineer** hat. The **overview** below is hand-written
and must stay within one screen; the **module/function map** is **generated** by
the check harness so it cannot drift (see [process.md §3/§7](process.md)).

## High-level flow

```
<input> ──► <stage 1> ──► <stage 2> ──► <output>
            (module)       (module)
```

_Describe the data flow in a few boxes/arrows. Keep it readable at a glance._

## Module responsibilities

| Module | Responsibility | Key public items |
|---|---|---|
| `path/to/mod_a` | <one line> | |
| `path/to/mod_b` | <one line> | |

Design rules (enforced): shared logic lives in one place (no duplication); pure,
unit-testable cores are separated from I/O / network / GUI shells; functions stay
small; each module has a single clear responsibility.

<!-- BEGIN GENERATED MODULE MAP -->
_(the harness regenerates the public-item map here)_
<!-- END GENERATED MODULE MAP -->
