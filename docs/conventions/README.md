# Conventions

Project conventions that are enforced mechanically (a CI gate and/or a
test), as distinct from the architectural decisions in
[`docs/adr`](../adr). An ADR records *why* a direction was chosen; a
convention here records a *rule new code must follow* and points at the
check that enforces it.

| Convention | Enforced by | Authority |
| --- | --- | --- |
| [No vague generic names](naming.md) | `cargo xtask lint-names` (CI **Naming** step) + an `xtask` workspace-scan test | [ADR 0003](../adr/0003-lint-and-code-quality-policy.md) §7 |
