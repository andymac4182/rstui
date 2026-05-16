# Conventions

Project conventions that are enforced mechanically (a CI gate and/or a
test), as distinct from the architectural decisions in
[`docs/adr`](../adr). An ADR records *why* a direction was chosen; a
convention here records a *rule new code must follow* and points at the
check that enforces it.

| Convention | Enforced by | Authority |
| --- | --- | --- |
| [No vague generic names](naming.md) | `cargo xtask lint-names` (CI **Naming** step) + an `xtask` workspace-scan test | [ADR 0003](../adr/0003-lint-and-code-quality-policy.md) §7 |

Every convention here and every CI gate runs at once, fail-fast, with
`cargo xtask ci` — the one command the [development loop](../development.md)
is built around.
