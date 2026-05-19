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

use crate::color::{ColorToken, Palette};
use crate::tree::{ChartKind, ChartSeries, UiNode};

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

    if series.is_empty() {
        return UiNode::Placeholder(format!("{kind:?}"));
    }
    UiNode::Chart {
        kind,
        series,
        cols,
        height,
    }
}
