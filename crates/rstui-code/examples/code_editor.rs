//! "It looks like a proper code editor with real colours."
//!
//! A multi-line Rust source rendered through the
//! [`rstui_code::Editor`] with its **syntax overlay produced by a real
//! tree-sitter parse** ([`rstui_code::Analyzer`]) — the ADR 0022
//! Tier-1 drop-in for the dependency-free Tier-0. One [`Analyzer`] parse
//! feeds *both* the per-cell colour overlay (`Editor::syntax`) *and* the
//! symbol [`Outline`](rstui_code::Outline) the side panel would show.
//!
//! Per the rstui example convention this is a **deterministic,
//! self-asserting [`TestBackend`] smoke** (not `run_app`): it renders once,
//! then asserts that specific glyphs carry the keyword / string / comment
//! colours (proving the colour came from a real parse, not a heuristic) and
//! that the outline found the expected `fn` / `struct` symbols. Running it
//! is the test:
//!
//! ```text
//! cargo run -p rstui-code --example code_editor
//! ```

use rstui_code::syntax::SyntaxStyles;
use rstui_code::{Analyzer, Editor, SymbolKind, TsLanguage};
use rstui_core::{Color, Position, Style, Terminal, TestBackend, TextArea};
use rstui_widgets::Block;

fn main() {
    // A realistic little Rust file: comments, keywords, a string, a number,
    // a struct and two fns (one a method) — enough to *look* like code.
    let source = "\
// A tiny module.
struct Point {
    x: i32,
    y: i32,
}

impl Point {
    fn origin() -> Point {
        Point { x: 0, y: 0 }
    }
}

fn main() {
    let p = Point::origin();
    let label = \"point\";
    println!(\"{label}: {} {}\", p.x, p.y);
}
";

    // The document the editor renders (rows joined by '\n' — exactly what
    // the analyzer parses).
    let doc = TextArea::from_value(source);

    // The four theme buckets. Distinct, easily-identified colours so the
    // assertions below can prove which classifier painted each glyph.
    let styles = SyntaxStyles {
        keyword: Style::new().fg(Color::Blue),
        string: Style::new().fg(Color::Green),
        number: Style::new().fg(Color::Magenta),
        comment: Style::new().fg(Color::DarkGray),
        // The richer Tier-1-only semantic classes default to no colour.
        ..Default::default()
    };

    // ONE tree-sitter parse → BOTH outputs (ADR 0022 driver 1).
    let mut analyzer = Analyzer::new(TsLanguage::Rust);
    analyzer.set_source(&doc.to_string());
    let overlay = analyzer.highlight(&styles); // drop-in for Editor::syntax
    let outline = analyzer.outline(); // rstui_code::Outline

    // The overlay is the flattened, newline-inclusive per-char layout the
    // Editor expects — a true drop-in.
    assert_eq!(
        overlay.len(),
        doc.to_string().chars().count(),
        "overlay must be the flattened drop-in layout Editor::syntax expects"
    );

    let mut terminal = Terminal::new(TestBackend::new(48, 20)).expect("TestBackend is infallible");
    terminal
        .draw(|frame| {
            let block = Block::bordered().title("code_editor — tree-sitter colours");
            frame.render_widget(
                Editor::new(&doc)
                    .focused(true)
                    .syntax(&overlay)
                    .block(block)
                    // Keep the caret/origin in view (the deferred seam — the
                    // app owns scroll; here the doc fits so it is (0, 0)).
                    .scroll(doc.scroll_into_view((0, 0), (46, 18), 2)),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    let buf = terminal.backend().buffer();

    // Find the first cell whose glyph is `ch` *and* whose fg is `want` —
    // proving that glyph got the expected bucket colour from the parse.
    let painted = |ch: char, want: Color| -> bool {
        let area = buf.area();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(c) = buf.get(Position::new(x, y)) {
                    if c.symbol == ch && c.fg == want {
                        return true;
                    }
                }
            }
        }
        false
    };

    // Real tree-sitter colour, proven on the rendered surface:
    // - the `s` of the `struct` keyword is keyword-blue,
    // - the `f` of `fn` is keyword-blue,
    // - a `p` from the `"point"` string literal is string-green,
    // - a `/` from the `// A tiny module.` line comment is comment-grey.
    assert!(
        painted('s', Color::Blue),
        "a keyword glyph (struct/…) must be keyword-coloured by tree-sitter"
    );
    assert!(
        painted('f', Color::Blue),
        "the `fn` keyword must be keyword-coloured by tree-sitter"
    );
    assert!(
        painted('p', Color::Green),
        "a glyph inside the \"point\" string literal must be string-coloured"
    );
    assert!(
        painted('/', Color::DarkGray),
        "the `//` line comment must be comment-coloured"
    );

    // The SAME parse produced the symbol outline the side panel would show.
    let names: Vec<(&str, SymbolKind)> = outline
        .0
        .iter()
        .map(|s| (s.name.as_str(), s.kind))
        .collect();
    assert!(
        names.contains(&("Point", SymbolKind::Struct)),
        "outline must find `struct Point` (got {names:?})"
    );
    assert!(
        names.contains(&("main", SymbolKind::Function)),
        "outline must find `fn main` (got {names:?})"
    );
    assert!(
        names.contains(&("Point", SymbolKind::Impl)),
        "outline must find the `impl Point` block (got {names:?})"
    );
    // `origin` is a method *inside* the `impl` → nested.
    let origin = outline
        .0
        .iter()
        .find(|s| s.name == "origin")
        .expect("outline must find `fn origin`");
    assert_eq!(origin.kind, SymbolKind::Method, "origin is a method");
    assert!(origin.depth >= 1, "origin nested inside `impl Point`");

    // Snapshot the rendered editor (deterministic — the smoke's artefact).
    print!("{}", terminal.backend());
    eprintln!(
        "tree-sitter outline: {:?}",
        outline
            .0
            .iter()
            .map(|s| (s.name.clone(), s.kind, s.line, s.depth))
            .collect::<Vec<_>>()
    );
}
