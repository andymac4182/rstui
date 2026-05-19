# ADR 0026: Calendar-app widget suite — a shared event model + pure-projection views

- **Status:** Accepted
- **Date:** 2026-05-19
- **Deciders:** rstui maintainers
- **Amends:** [ADR 0012](0012-widget-composition-and-layout-model.md) (adds
  the calendar family to the pure-projection catalog; does not change the
  model), and applies [ADR 0002](0002-widget-crate-boundary.md) §4 (the
  no-heavy-dependency rule) to date/time

## Context

The catalog had a date-only [`Calendar`] and a `DatePicker`, but nothing to
build an actual calendar *application*: no month grid that carries events, no
day/week time grids, no agenda or year overview, no way to detail, schedule,
edit, or move an event. Building one naïvely invites two mistakes the rest of
the framework has already ruled out:

1. **Pulling in `chrono`/`time`.** A calendar is the textbook reason to reach
   for a date crate — weekday computation, month lengths, time arithmetic.
   ADR 0002 §4 gates any transitive dependency behind a Cargo feature, and
   [`Calendar`]/[`Gantt`] already proved a scheduling surface needs *none* of
   it: the caller (its reducer, or a date crate of *its* choosing) owns the
   numbers; the widget only lays them out.
2. **Six widgets, six event structs.** Month/week/day/agenda/year/detail all
   consume "an event". Re-inventing that struct per widget — and the
   overlap-tiling maths the time grids each need — is the divergence ADR 0012
   warns against.

## Decision

A **one model, many pure-projection views** suite, every piece obeying the
existing discipline (no date math, total, no new dependency, caller owns all
state and all interaction).

- **One shared model — `rstui_widgets::event`.** `CalendarEvent` carries an
  opaque caller `id`/title, an inclusive `[day, end_day]` span on a
  *caller-chosen integer day axis* (days-since-epoch, day-of-month, a column
  index — the model never interprets the unit, exactly the [`Gantt`] axis
  rule), a `[start_min, end_min]` minute-of-day, an `all_day` flag, an accent
  colour, and optional location/description. It is a **model, not a widget**
  (the [`Link`] precedent): the only behaviour is `pack_day`, the
  interval-partitioning **overlap-column packer** the week/day grids share,
  and `time_label`/`time_label_12h` (pure *clock* arithmetic on a caller
  integer — not calendar math, the same justified-arithmetic line
  [`Calendar`]'s `{day:>2}` sits on). No `chrono`/`time`; no feature gate.
- **Nine views/controls**, each a pure projection borrowing
  `&[CalendarEvent]` + caller-owned selection/scroll/open state, each
  *total* (the Gauge rule: empty/clamped/out-of-range inputs are safe
  no-ops, never a panic), each with a self-asserting `*_demo` and exhaustive
  `TestBackend` snapshot tests:
  `MonthView`, `WeekView`, `DayView`, `AgendaView`, `YearView` (reuses
  [`Calendar`] per mini-month), `TimePicker` (the [`DatePicker`]/[`Select`]
  anchored-opaque-panel idiom), `EventCard` (one-event detail body, framed by
  a [`Modal`]/popover at the call site), `EventEditor` (the [`Form`] pattern:
  pure *layout* projection owning no app state, exposing each control's
  `Rect` via `field_rect`), `DateNavigator` (the [`Tabs`]/[`StatusBar`]
  one-row strip).
- **The app owns add / move / schedule / click — not the widgets.** Per
  ADR 0012 a click/drag is a *projection*, not a callback: each view exposes
  pure hit accessors that invert its render walk (`day_at`/`event_at`,
  `slot_at`/`minute_at`, `target_at`, `field_rect`) computed from the same
  shared layout `render` uses, so what is drawn and what a click resolves to
  can never drift. Moving an event is the documented press→drag→release
  pointer-gesture recipe (the Kanban-board / git-review precedent): the
  reducer reads `slot_at(release_pos)` and rewrites the event; the widget
  stays read-only. Scheduling/editing is `Modal` + `EventEditor` + the
  caller's own `Input`/`Switch`/`DatePicker`/`TimePicker`/`Select` rendered
  into `field_rect`s.

## Consequences

- A full calendar app is now composable from the catalog with zero new
  dependencies; the kitchen-sink gains a flagship **Calendar** experience
  (Month/Week/Day/Agenda/Year switchable via `DateNavigator`, click-an-event
  → `Modal`+`EventCard`, new/edit → `EventEditor`, drag-to-move via the
  pointer seam) and the view widgets are surfaced in the Data-display tour.
- The overlap-tiling maths lives once (`event::pack_day`, deterministic,
  unit-tested) instead of three divergent copies in the time grids.
- `cargo xtask ci` (all 5 gates) stays green; `rstui-widgets` keeps its
  single `rstui-core` dependency. The suite is purely additive — no existing
  widget, signature, or render output changed.
- Deliberately deferred (additive, not smuggled in): recurrence-rule
  expansion, time-zone handling, and localized month/weekday label sets —
  all caller-or-future-ADR concerns, kept out of the model exactly as
  [`Calendar`] deferred localization.

[`Calendar`]: ../widgets/forms-and-data.md
[`Gantt`]: ../widgets/charts.md
[`Link`]: ../widgets/rich-rendering.md
[`DatePicker`]: ../widgets/forms-and-data.md
[`Select`]: ../widgets/forms-and-data.md
[`Modal`]: ../widgets/overlays-and-control.md
[`Form`]: ../widgets/forms-and-data.md
[`Tabs`]: ../widgets/navigation-and-layout.md
[`StatusBar`]: ../widgets/navigation-and-layout.md
