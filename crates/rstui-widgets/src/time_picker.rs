//! Placeholder — replaced by the calendar-widget-suite build.
use rstui_core::{Buffer, Rect, Widget};

/// Placeholder for the closed HH:MM time field with a dropped time list.
#[derive(Debug, Default, Clone)]
pub struct TimePicker;

impl Widget for TimePicker {
    fn render(self, _area: Rect, _buf: &mut Buffer) {}
}
