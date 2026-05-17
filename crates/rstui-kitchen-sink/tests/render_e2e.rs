//! App-scale rendering + colour E2E: drive the *exact* `KitchenSink` app the
//! binary runs, visit every screen, and assert it not only renders the right
//! glyphs but actually applies colour through the real theme — checked on the
//! rendered `Cell`s, which the glyph-only `snapshot()` cannot see. This pairs
//! with `rstui-smoke`'s primitive cascade tests: there the colour model, here
//! the whole composed application.

use std::collections::HashSet;

use rstui_core::{Color, Event, KeyCode, KeyEvent, Position, Size};
use rstui_kitchen_sink::KitchenSink;
use rstui_runtime::Harness;

fn harness() -> Harness<KitchenSink> {
    Harness::new(KitchenSink::new(Size::new(120, 40)), 120, 40)
}
fn ch(c: char) -> Event {
    Event::from(KeyEvent::char(c))
}
fn key(code: KeyCode) -> Event {
    Event::from(KeyEvent::from_code(code))
}

/// Distinct `(fg, bg)` colour pairs across the whole rendered frame. A purely
/// monochrome (uncoloured) frame yields exactly one pair; a themed UI yields
/// many — the end-to-end proof that colour reaches the cells.
fn distinct_color_pairs(h: &Harness<KitchenSink>) -> HashSet<(Color, Color)> {
    let buf = h.backend().buffer();
    let a = buf.area();
    let mut set = HashSet::new();
    for y in a.top()..a.bottom() {
        for x in a.left()..a.right() {
            if let Some(c) = buf.get(Position::new(x, y)) {
                set.insert((c.fg, c.bg));
            }
        }
    }
    set
}

#[test]
fn every_screen_renders_with_colour_and_never_panics() {
    // (hotkey, a stable marker the screen renders)
    let screens = [
        ('1', "Welcome"),
        ('2', "Forms"),
        ('3', "Navigation"),
        ('4', "Data"),
        ('5', "Feedback"),
        ('6', "Containers"),
        ('7', "Rich Text"),
        ('8', "Colour"),
    ];
    for (hotkey, marker) in screens {
        let mut h = harness();
        h.handle(ch(hotkey));
        assert!(h.is_running(), "screen {marker} keeps the app running");

        let snap = h.snapshot();
        assert!(
            snap.contains(marker),
            "screen {marker} must render its content; got:\n{snap}"
        );
        // The frame is not blank.
        assert!(
            snap.chars().any(|c| !c.is_whitespace()),
            "screen {marker} rendered a blank frame"
        );
        // …and it is actually coloured: the real theme paints more than one
        // (fg,bg) pair (chrome accent + content + footer at minimum).
        let pairs = distinct_color_pairs(&h);
        assert!(
            pairs.len() >= 3,
            "screen {marker} only used {} colour pair(s) — theme not applied",
            pairs.len()
        );
    }
}

#[test]
fn colour_lab_paints_a_rich_palette() {
    let mut h = harness();
    h.handle(ch('8')); // Colour Lab
    assert!(h.snapshot().contains("Indexed(0)"), "cursor label renders");

    // A colour-palette screen must show *many* distinct backgrounds — this is
    // the screen whose entire purpose is colour, so a weak palette here means
    // the colour pipeline regressed end to end.
    let bgs: HashSet<Color> = {
        let buf = h.backend().buffer();
        let a = buf.area();
        let mut s = HashSet::new();
        for y in a.top()..a.bottom() {
            for x in a.left()..a.right() {
                if let Some(c) = buf.get(Position::new(x, y)) {
                    s.insert(c.bg);
                }
            }
        }
        s
    };
    assert!(
        bgs.len() >= 12,
        "Colour Lab should paint a wide palette; only {} distinct bg colours",
        bgs.len()
    );
}

#[test]
fn rendering_survives_navigation_resize_and_ticks_across_all_screens() {
    // Walk every screen, resize between them, advance the animation clock —
    // the whole composed render path must stay total (no panic) and keep
    // running and coloured at every size.
    let mut h = harness();
    for (i, hotkey) in "12345678".chars().enumerate() {
        h.handle(ch(hotkey));
        // Alternate a few terminal sizes, including a deliberately tiny one.
        let (w, hgt) = match i % 3 {
            0 => (120, 40),
            1 => (60, 20),
            _ => (8, 4),
        };
        h.resize(w, hgt);
        h.handle(key(KeyCode::Down));
        h.handle(key(KeyCode::Right));
        for _ in 0..3 {
            h.tick();
        }
        assert!(
            h.is_running(),
            "screen {hotkey} at {w}x{hgt} survived input+resize+ticks"
        );
        assert!(
            !h.snapshot().is_empty(),
            "screen {hotkey} at {w}x{hgt} still renders"
        );
    }
    // Back to a normal size, still coloured and running.
    h.resize(120, 40);
    h.handle(ch('1'));
    assert!(h.is_running());
    assert!(
        distinct_color_pairs(&h).len() >= 3,
        "theme intact after the tour"
    );
}
