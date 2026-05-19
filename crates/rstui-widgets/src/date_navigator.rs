//! Placeholder — replaced by the calendar-widget-suite build.
use rstui_core::{Buffer, Rect, Widget};

/// Placeholder for the calendar-app toolbar.
#[derive(Debug, Default, Clone)]
pub struct DateNavigator;

/// Placeholder hit-test target enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavTarget {
    /// The previous-period control.
    Prev,
}

impl Widget for DateNavigator {
    fn render(self, _area: Rect, _buf: &mut Buffer) {}
}
