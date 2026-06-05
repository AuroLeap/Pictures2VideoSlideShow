# User Needs (UN-###)

Owned by the **End User** hat. Plain-language needs + edge-case expectations.
Engineering translations live in `system-requirements.csv` (referenced by
`UN-Refs`); do not restate them here. Priority: **M**=Must · **S**=Should ·
**C**=Could.

## Core needs

| UN-ID | Need (plain language) | Why it matters | Priority | Acceptance intent (how we'd know it's met) |
|---|---|---|---|---|
| UN-001 | | | | |

## Edge-case expectations

How the system should behave when things go wrong (the highest-value part — be
specific; the System Engineer turns each into measurable SRs).

| UN-ID | Scenario | Expected behavior |
|---|---|---|
| UN-0xx | Interruption / power loss / killed mid-operation | |
| UN-0xx | Invalid / corrupt / unsupported input | |
| UN-0xx | Resource exhaustion (disk / memory full) | |
| UN-0xx | Missing dependency / wrong version | |
| UN-0xx | Output target removed / locked / unwritable | |
| UN-0xx | Unattended/automated run (must never block; clear failure) | |
| UN-0xx | First-run setup & discoverable docs / quick-reference | |
