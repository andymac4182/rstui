//! Format-agnostic chart-data parsing: a JSON `data`/`series` shape (the
//! LLM-friendly forms an A2UI or json-render agent emits) → a themed
//! [`UiNode::Chart`].
//!
//! Both format layers ([`a2ui`](crate::a2ui) /
//! [`jsonrender`](crate::jsonrender)) call [`build_chart`] so the data
//! shapes and the palette/series-cycling rules stay identical. Series
//! colours are resolved here against the active
//! [`Palette`] (an explicit `"color"` token, a per-series token, else a
//! cycled `chart_1..=chart_5`), keeping [`crate::tree`] free of any JSON
//! dependency (it carries only resolved values — ADR 0012).

use serde_json::Value;

use rstui_core::{Alignment, Color, Modifier, Style};

use crate::color::{ColorToken, Palette};
use crate::tree::{ChartKind, ChartSeries, TextVariant, UiNode};

/// A **visible** chart diagnostic — never a silent blank or a terse
/// `[unsupported]`. The user/agent sees exactly why nothing was drawn
/// and the data shape that would fix it (a bordered, warning-styled
/// box so it cannot be mistaken for the chart itself).
#[must_use]
fn chart_diagnostic(kind: ChartKind, reason: &str, expected: &str) -> UiNode {
    let warn = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let dim = Style::new().add_modifier(Modifier::DIM);
    UiNode::Card {
        title: Some(format!("⚠ {kind:?} chart not rendered")),
        child: Box::new(UiNode::Column {
            children: vec![
                UiNode::Text {
                    spans: vec![(reason.to_owned(), warn)],
                    variant: TextVariant::Body,
                    align: Alignment::Left,
                    wrap: true,
                },
                UiNode::Text {
                    spans: vec![(format!("expected: {expected}"), dim)],
                    variant: TextVariant::Caption,
                    align: Alignment::Left,
                    wrap: true,
                },
            ],
            justify: crate::tree::Justify::Start,
            align: crate::tree::CrossAlign::Stretch,
        }),
    }
}

/// Parse a chart data array into `(points, labels)`. Accepts
/// `[{label,value}|{x,y}|[x,y]|number]` — the LLM-friendly shapes; `x`
/// defaults to the index, missing labels are empty. Total.
#[must_use]
pub fn parse_points(data: Option<&Value>) -> (Vec<(f64, f64)>, Vec<String>) {
    let Some(Value::Array(items)) = data else {
        return (Vec::new(), Vec::new());
    };
    let mut points = Vec::with_capacity(items.len());
    let mut labels = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let idx = i as f64;
        let (x, y, label) = match item {
            Value::Number(_) => (idx, item.as_f64().unwrap_or(0.0), String::new()),
            Value::Array(pair) => (
                pair.first().and_then(Value::as_f64).unwrap_or(idx),
                pair.get(1).and_then(Value::as_f64).unwrap_or(0.0),
                String::new(),
            ),
            Value::Object(map) => {
                let y = map
                    .get("value")
                    .or_else(|| map.get("y"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                let x = map.get("x").and_then(Value::as_f64).unwrap_or(idx);
                let label = map
                    .get("label")
                    .or_else(|| map.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                (x, y, label)
            }
            _ => continue,
        };
        points.push((x, y));
        labels.push(label);
    }
    (points, labels)
}

/// Build a themed [`UiNode::Chart`] from the resolved `series`/`data`
/// values. `series_val` (`[{name,color?,data|points}]`) is multi-series;
/// otherwise `data_val` (`[{label,value}|…]`) is a single series — and
/// for [`ChartKind::Pie`] becomes one cycled-colour series per slice.
/// `explicit` is a chart-wide `"color"` token; an absent series colour
/// cycles `chart_1..=chart_5`. Empty data → a visible placeholder.
#[must_use]
#[allow(clippy::cast_precision_loss, clippy::too_many_arguments)]
pub fn build_chart(
    kind: ChartKind,
    series_val: Option<&Value>,
    data_val: Option<&Value>,
    label: Option<&str>,
    explicit: Option<ColorToken>,
    height: u16,
    cols: usize,
    palette: &Palette,
) -> UiNode {
    let mut series: Vec<ChartSeries> = Vec::new();

    if let Some(Value::Array(arr)) = series_val {
        for (i, s) in arr.iter().enumerate() {
            let color = s
                .get("color")
                .and_then(Value::as_str)
                .and_then(crate::color::parse_token)
                .or(explicit)
                .map_or_else(|| palette.series(i), |token| palette.resolve(token));
            let (points, labels) = parse_points(s.get("data").or_else(|| s.get("points")));
            series.push(ChartSeries {
                name: s
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                color,
                points,
                labels,
            });
        }
    } else {
        let (points, labels) = parse_points(data_val);
        if matches!(kind, ChartKind::Pie) {
            for (i, (&(_, y), slice_label)) in points.iter().zip(&labels).enumerate() {
                series.push(ChartSeries {
                    name: slice_label.clone(),
                    color: explicit
                        .map_or_else(|| palette.series(i), |token| palette.resolve(token)),
                    points: vec![(i as f64, y)],
                    labels: vec![slice_label.clone()],
                });
            }
        } else if !points.is_empty() {
            series.push(ChartSeries {
                name: label.unwrap_or("").to_owned(),
                color: explicit.map_or_else(|| palette.series(0), |token| palette.resolve(token)),
                points,
                labels,
            });
        }
    }

    // Diagnostics — the user must see WHY a chart did not render, never
    // a silent blank or a terse "[unsupported]".
    let total_points: usize = series.iter().map(|s| s.points.len()).sum();
    if series.is_empty() || total_points == 0 {
        return chart_diagnostic(
            kind,
            "no numeric data was supplied",
            "props.data:[{\"label\":…,\"value\":N}] or \
             props.series:[{\"name\":…,\"points\":[[x,y],…]}]",
        );
    }
    // The categorical widgets (Bar/Sparkline/Histogram/StackedBar/Pie)
    // plot non-negative magnitudes: a series that is entirely ≤ 0 or
    // non-finite has nothing to draw — say so rather than blank out.
    let categorical = matches!(
        kind,
        ChartKind::Bar
            | ChartKind::Sparkline
            | ChartKind::Histogram
            | ChartKind::StackedBar
            | ChartKind::Pie
    );
    if categorical
        && !series
            .iter()
            .flat_map(|s| s.points.iter())
            .any(|&(_, y)| y.is_finite() && y > 0.0)
    {
        return chart_diagnostic(
            kind,
            "every value is ≤ 0 or non-numeric — nothing to plot",
            "at least one positive numeric value (this chart can't show negatives)",
        );
    }
    UiNode::Chart {
        kind,
        series,
        cols,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::HitMap;
    use rstui_core::{Buffer, Position, Rect};
    use serde_json::json;

    fn nonblank(node: &UiNode, w: u16, h: u16) -> usize {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        node.render(buf.area(), &mut buf, &mut HitMap::new());
        (0..w)
            .flat_map(|x| (0..h).map(move |y| (x, y)))
            .filter(|&(x, y)| {
                buf.get(Position::new(x, y))
                    .is_some_and(|c| c.symbol != ' ')
            })
            .count()
    }

    #[test]
    fn fractional_data_renders_not_blank() {
        // The reported bug: a normalized/percentage series (every value
        // < 1) used to truncate f64→u64 to all-zero → a blank chart.
        let data = json!([
            {"label":"A","value":0.42},
            {"label":"B","value":0.81},
            {"label":"C","value":0.6}
        ]);
        let node = build_chart(
            ChartKind::Bar,
            None,
            Some(&data),
            None,
            None,
            10,
            0,
            &Palette::ANSI,
        );
        assert!(
            matches!(node, UiNode::Chart { .. }),
            "fractional data must still be a chart, got {node:?}"
        );
        assert!(
            nonblank(&node, 40, 10) > 12,
            "fractional bars must actually draw (was ~2 = blank)"
        );
        // Relative heights preserved: B (0.81) tallest, A (0.42) shortest.
        let UiNode::Chart { series, .. } = &node else {
            unreachable!()
        };
        assert_eq!(series[0].points.len(), 3);
    }

    #[test]
    fn missing_or_nonpositive_data_shows_a_visible_diagnostic() {
        // No data, all-zero, and non-numeric must each render a clear,
        // bordered reason — never a silent blank or a terse
        // "[unsupported]" (the user asked to see *why*).
        for (name, data) in [
            ("none", None),
            ("empty", Some(json!([]))),
            ("all-zero", Some(json!([{"label":"a","value":0}]))),
            ("negative", Some(json!([{"label":"a","value":-3}]))),
        ] {
            let node = build_chart(
                ChartKind::Bar,
                None,
                data.as_ref(),
                None,
                None,
                10,
                0,
                &Palette::ANSI,
            );
            let UiNode::Card { title, .. } = &node else {
                panic!("{name}: expected a diagnostic Card, got {node:?}");
            };
            assert!(
                title.as_deref().unwrap_or("").contains("not rendered"),
                "{name}: the diagnostic names the failure: {title:?}"
            );
            assert!(
                nonblank(&node, 50, 10) > 20,
                "{name}: the diagnostic is actually visible"
            );
        }
    }

    #[test]
    fn line_chart_keeps_real_axes_no_scaling_diagnostic() {
        // Line/Scatter use real f64 axes — negative/zero is valid data,
        // not a "nothing to plot" diagnostic.
        let series = json!([{"name":"s","points":[[0,-2],[1,0],[2,5]]}]);
        let node = build_chart(
            ChartKind::Line,
            Some(&series),
            None,
            None,
            None,
            10,
            0,
            &Palette::ANSI,
        );
        assert!(
            matches!(
                node,
                UiNode::Chart {
                    kind: ChartKind::Line,
                    ..
                }
            ),
            "a line chart with negatives is still a chart, got {node:?}"
        );
    }
}
