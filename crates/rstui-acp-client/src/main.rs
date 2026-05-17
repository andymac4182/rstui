//! `rstui-acp-client` — the full-screen ACP chat client binary.
//!
//! ```text
//! rstui-acp-client                       # open the registry agent picker
//! rstui-acp-client --agent "npx -y @zed-industries/claude-code-acp@latest"
//! rstui-acp-client --plugin ./my-plugin  # attach an extra plugin (repeatable)
//! ```
//!
//! With no `--plugin`, the reference plugins next to this binary
//! (`rstui-acp-plugin-powerline`, `-btw`, `-ask-user`) are auto-attached so
//! the powerline footer and slash commands work out of the box.

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return ExitCode::SUCCESS;
    }

    let config = rstui_acp_client::Config::from_args(args);
    match rstui_acp_client::run(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            rstui_crossterm::restore_terminal();
            eprintln!("rstui-acp-client: {err}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    println!(
        "rstui-acp-client — a full-screen ACP chat client\n\n\
         USAGE:\n  rstui-acp-client [--agent <cmd>] [--plugin <cmd>]…\n\n\
         OPTIONS:\n\
         \x20 --agent  <cmd>   Launch this agent command directly (skip the picker)\n\
         \x20 --plugin <cmd>   Attach an extension plugin process (repeatable)\n\
         \x20 -h, --help       Show this help\n\n\
         With no --agent the ACP registry picker opens. With no --plugin the\n\
         in-tree reference plugins beside this binary are auto-attached."
    );
}
