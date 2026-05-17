//! End-to-end fidelity gate for the gpui-component port.
//!
//! These tests assert the three things that can silently break a faithful
//! port: every vendored theme resolves and every colour comes out *opaque*
//! (the composite step ran), known literal colours land on their exact RGB,
//! and the derived-fallback + alpha-clamp chain reproduces gpui's output.

use rstui_core::Color;
use rstui_theme::Theme;

/// The colour fields that must be populated for *every* theme: a spread of
/// base (`background`), no-fallback (`foreground`), blended-fallback
/// (`muted_foreground`), darken-fallback (`primary_active`), mode-dependent
/// (`group_box`), and alpha-clamped (`selection`) cases. If any stayed
/// transparent the cascade or composite skipped it.
fn critical_colors(p: &rstui_theme::ThemePalette) -> Vec<(&'static str, Color)> {
    vec![
        ("background", p.background),
        ("foreground", p.foreground),
        ("border", p.border),
        ("primary", p.primary),
        ("primary_foreground", p.primary_foreground),
        ("primary_active", p.primary_active),
        ("secondary", p.secondary),
        ("muted_foreground", p.muted_foreground),
        ("accent", p.accent),
        ("danger", p.danger),
        ("success", p.success),
        ("warning", p.warning),
        ("info", p.info),
        ("ring", p.ring),
        ("selection", p.selection),
        ("list_active", p.list_active),
        ("scrollbar_thumb", p.scrollbar_thumb),
        ("group_box", p.group_box),
        ("red", p.red),
        ("green", p.green),
        ("blue", p.blue),
        ("yellow", p.yellow),
        ("cyan", p.cyan),
        ("magenta", p.magenta),
        ("red_light", p.red_light),
        ("cyan_light", p.cyan_light),
    ]
}

#[test]
fn catalogue_resolves_and_every_colour_is_opaque_rgb() {
    let all = Theme::all();
    // 21 sets, several with light+dark (Catppuccin alone has 4) — comfortably
    // more than the set count.
    assert!(all.len() >= 21, "got {} themes", all.len());

    for fam in ["Catppuccin", "Tokyo", "Gruvbox", "Solarized", "Ayu"] {
        assert!(
            all.iter().any(|t| t.name.contains(fam)),
            "missing a {fam} theme"
        );
    }

    for t in &all {
        assert!(!t.name.is_empty(), "a theme has no name");
        for (field, c) in critical_colors(&t.palette) {
            // The composite step always yields a concrete 24-bit colour;
            // anything else means a field never got resolved.
            assert!(
                matches!(c, Color::Rgb(..)),
                "{} field {field} is {c:?}, not an opaque Rgb",
                t.name
            );
        }
    }
}

#[track_caller]
fn assert_near(got: Color, want: (u8, u8, u8), tol: i32, what: &str) {
    let Color::Rgb(r, g, b) = got else {
        panic!("{what}: expected Rgb, got {got:?}");
    };
    let (wr, wg, wb) = want;
    let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).abs();
    assert!(
        d(r, wr) <= tol && d(g, wg) <= tol && d(b, wb) <= tol,
        "{what}: got ({r},{g},{b}), want ~({wr},{wg},{wb}) (tol {tol})"
    );
}

#[test]
fn twilight_literal_overrides_land_on_exact_rgb() {
    let tw = Theme::by_name("Twilight").expect("Twilight is vendored");
    assert!(tw.palette.is_dark());
    // Pure greys round-trip exactly: background #141414, foreground #dcdcdc,
    // border #343434.
    assert_near(tw.palette.background, (0x14, 0x14, 0x14), 0, "twilight bg");
    assert_near(tw.palette.foreground, (0xdc, 0xdc, 0xdc), 0, "twilight fg");
    assert_near(tw.palette.border, (0x34, 0x34, 0x34), 0, "twilight border");
}

#[test]
fn twilight_derived_fallback_matches_gpui() {
    let tw = Theme::by_name("Twilight").unwrap();
    // Twilight sets no `ring`; gpui's fallback is `self.blue`, and Twilight's
    // `base.blue` is #44474a. ring is opaque (a=1) so it's that colour,
    // within HSL round-trip rounding.
    assert_near(
        tw.palette.ring,
        (0x44, 0x47, 0x4a),
        2,
        "twilight ring=base.blue",
    );
}

#[test]
fn alpha_clamps_keep_selection_and_active_subtle() {
    // Twilight's `primary` (#CDA869, a warm gold) feeds `selection`, which
    // gpui clamps to a<=0.3 then we composite over the #141414 background.
    // The result must read as "near background", nowhere near solid gold.
    let tw = Theme::by_name("Twilight").unwrap();
    let Color::Rgb(r, g, b) = tw.palette.selection else {
        panic!("rgb");
    };
    assert!(
        r < 90 && g < 80 && b < 60,
        "selection should be a faint wash over #141414, got ({r},{g},{b})"
    );
    // `list.active.background` is #CDA86911 (~7% gold): even fainter.
    let Color::Rgb(lr, lg, lb) = tw.palette.list_active else {
        panic!("rgb");
    };
    assert!(
        lr < 45 && lg < 40 && lb < 35,
        "list_active ~7% gold over near-black, got ({lr},{lg},{lb})"
    );
}

#[test]
fn user_json_resolves_against_the_built_in_base() {
    // A minimal user theme: override only two colours; everything else must
    // fall back to the dark canonical base (proving user files share the
    // exact resolution path, not a blank default).
    let json = r##"{
        "name": "My Set",
        "themes": [
            { "name": "Mine", "mode": "dark",
              "colors": { "background": "#101820", "primary.background": "#ff8800" } }
        ]
    }"##;
    let themes = Theme::from_set_json(json).expect("valid user theme");
    assert_eq!(themes.len(), 1);
    let p = &themes[0].palette;
    assert_eq!(themes[0].set_name, "My Set");
    assert_near(p.background, (0x10, 0x18, 0x20), 0, "user bg override");
    assert_near(p.primary, (0xff, 0x88, 0x00), 1, "user primary override");
    // Unset `border` came from the canonical dark base: opaque and clearly
    // distinct from the (very dark) background.
    assert!(matches!(p.border, Color::Rgb(..)), "border resolved");
    assert_ne!(
        p.border, p.background,
        "border fell back to the base, not bg"
    );

    // Malformed JSON is a typed error, not a panic.
    assert!(Theme::from_set_json("{ not json").is_err());
}

#[test]
fn by_name_is_case_insensitive_and_enumerable() {
    assert!(Theme::by_name("tWiLiGhT").is_some());
    assert!(Theme::by_name("no such theme").is_none());
    // Print the catalogue so `--nocapture` shows exactly what shipped.
    let all = Theme::all();
    eprintln!("--- {} built-in themes ---", all.len());
    for t in &all {
        eprintln!(
            "{:<5} {}",
            if t.palette.is_dark() { "dark" } else { "light" },
            t.name
        );
    }
}

#[test]
fn default_dark_is_usable_without_lookup() {
    let d = Theme::default_dark();
    assert!(d.palette.is_dark());
    assert!(matches!(d.palette.background, Color::Rgb(..)));
    // Its style constructors produce concrete styles.
    let sel = d.palette.selection();
    assert!(sel.fg.is_some() && sel.bg.is_some());
}
