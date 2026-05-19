//! Placeholder — replaced by the calendar-widget-suite build.
use rstui_core::{Buffer, Rect, Widget};

/// Placeholder for the single-day time-grid view.
#[derive(Debug, Default, Clone)]
pub struct DayView;

impl Widget for DayView {
    fn render(self, _area: Rect, _buf: &mut Buffer) {}
}
