//! Exercises [`LogStream`]: a framed projection of caller-owned OpenTelemetry-ish
//! [`LogRecord`]s spanning every [`LogLevel`], each with a timestamp and target
//! gutter, scrolled to the top.
//!
//! The records are plain caller-owned state — what an app's model holds and a
//! reducer recomputes; [`LogStream`] only reads them and the caller-owned
//! scroll offset (the pure projection [`List`]/[`BarChart`] use). Running over
//! a [`TestBackend`] keeps it TTY-free, so it doubles as a deterministic
//! snapshot smoke test:
//!
//! ```text
//! cargo run -p rstui-widgets --example log_stream_demo
//! ```

use rstui_core::{Style, Terminal, TestBackend};
use rstui_widgets::{Block, LogLevel, LogRecord, LogStream};

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).expect("TestBackend is infallible");

    // The structured log records an app's model would own.
    let records = || {
        [
            LogRecord::new(LogLevel::Info, "service starting (build a1b2c3)")
                .timestamp("12:00:00.001")
                .target("otel.boot"),
            LogRecord::new(LogLevel::Debug, "config loaded: 14 keys")
                .timestamp("12:00:00.018")
                .target("otel.config"),
            LogRecord::new(LogLevel::Trace, "span open: handle_request")
                .timestamp("12:00:00.042")
                .target("http.trace"),
            LogRecord::new(LogLevel::Info, "GET /api/widgets 200 12ms")
                .timestamp("12:00:00.055")
                .target("http.access"),
            LogRecord::new(LogLevel::Warn, "slow query: 138ms over budget")
                .timestamp("12:00:00.193")
                .target("db.pool"),
            LogRecord::new(LogLevel::Info, "cache warm: 2048 entries")
                .timestamp("12:00:00.210")
                .target("cache"),
            LogRecord::new(LogLevel::Error, "upstream timeout: payments")
                .timestamp("12:00:00.512")
                .target("net.client"),
            LogRecord::new(LogLevel::Warn, "retrying connection (attempt 2)")
                .timestamp("12:00:00.640")
                .target("net.client"),
            LogRecord::new(LogLevel::Debug, "circuit breaker half-open")
                .timestamp("12:00:00.901")
                .target("net.breaker"),
            LogRecord::new(LogLevel::Info, "all gates green")
                .timestamp("12:00:01.004")
                .target("merge.check"),
        ]
    };
    let records = records();

    terminal
        .draw(|frame| {
            // A framed stream of all five levels, scrolled to the top; the
            // scroll offset is plain caller-owned state the widget only reads.
            frame.render_widget(
                LogStream::new(&records)
                    .offset(0)
                    .message_style(Style::new())
                    .block(Block::bordered().title("application.log")),
                frame.area(),
            );
        })
        .expect("TestBackend is infallible");

    print!("{}", terminal.backend());
}
