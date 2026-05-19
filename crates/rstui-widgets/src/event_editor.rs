//! Placeholder — replaced by the calendar-widget-suite build.
use rstui_core::{Buffer, Rect, Widget};

/// Placeholder for the create/edit-event dialog layout.
#[derive(Debug, Default, Clone)]
pub struct EventEditor;

/// Placeholder field enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventEditorField {
    /// The title field.
    Title,
}

impl Widget for EventEditor {
    fn render(self, _area: Rect, _buf: &mut Buffer) {}
}
