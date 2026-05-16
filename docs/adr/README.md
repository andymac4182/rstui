# Architecture Decision Records

This directory records the architectural decisions that shape rstui:
the choices that are expensive to reverse, constrain later work, or
would otherwise have to be re-derived from scratch each time someone
asks "why is it built this way?".

An ADR is a short, dated, immutable document. It captures the context
that forced a decision, the options considered, the decision taken, and
the consequences accepted. Decisions are not edited after the fact — if
one is revisited, a new ADR supersedes it and links back.

## Format

Each record is `NNNN-kebab-title.md` and follows a lightweight
MADR-style shape:

- **Status** — Proposed, Accepted, Superseded by `NNNN`.
- **Context** — the forces in play, including constraints already
  locked in by earlier slices.
- **Decision drivers** — the axes the decision is judged on.
- **Options considered** — each with its real tradeoffs.
- **Decision** — the choice, stated plainly, with rationale.
- **Evidence** — concrete facts from the reference projects, cited, so
  the reasoning can be audited rather than taken on faith.
- **Consequences** — what this makes easy, what it makes hard, and what
  is deliberately deferred.

## Index

| ADR | Title | Status |
| --- | ----- | ------ |
| [0001](0001-terminal-backend-strategy.md) | Terminal backend strategy | Accepted |
| [0002](0002-widget-crate-boundary.md) | Widget crate boundary | Accepted |
| [0003](0003-lint-and-code-quality-policy.md) | Lint and code-quality policy | Accepted |
| [0004](0004-focus-routing-architecture.md) | Focus-routing architecture | Accepted |
