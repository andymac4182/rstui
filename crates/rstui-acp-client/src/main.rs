//! `rstui-acp-client` — the full-screen ACP chat client binary.
//!
//! ```text
//! rstui-acp-client                       # open the registry agent picker
//! rstui-acp-client --agent "npx -y @zed-industries/claude-code-acp@latest"
//! rstui-acp-client --cmd "python my_acp_server.py"  # any custom local-stdio ACP command
//! RSTUI_ACP_AGENT="./target/debug/my-acp" rstui-acp-client  # …or via the env var
//! rstui-acp-client --plugin ./my-plugin  # attach an extra plugin (repeatable)
//! ```
//!
//! `--agent`, `--cmd` and `--command` are synonyms: the **custom ACP
//! command** to launch and speak to over local stdio. Without one (and
//! without `RSTUI_ACP_AGENT`) the registry picker opens — which also offers
//! a "Custom command…" entry. With no `--plugin`, the reference plugins
//! next to this binary (`rstui-acp-plugin-powerline`, `-btw`, `-ask-user`)
//! are auto-attached so the powerline footer and slash commands work out of
//! the box.

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
         USAGE:\n  rstui-acp-client [--cmd <acp-command>] [--plugin <cmd>]…\n\n\
         OPTIONS:\n\
         \x20 --cmd <cmd>      Custom ACP command to run & speak to over local\n\
         \x20                  stdio (skip the picker). --agent and --command\n\
         \x20                  are synonyms.\n\
         \x20 --plugin <cmd>   Attach an extension plugin process (repeatable)\n\
         \x20 -h, --help       Show this help\n\n\
         ENV:\n\
         \x20 RSTUI_ACP_AGENT  Custom ACP command when no --cmd is given\n\n\
         With no command (and no RSTUI_ACP_AGENT) the ACP registry picker\n\
         opens — it also has a \"Custom command…\" entry. With no --plugin the\n\
         in-tree reference plugins beside this binary are auto-attached.\n\n\
         Via cargo (note the `--` before the switch):\n\
         \x20 cargo run -p rstui-acp-client -- --cmd \"python my_acp.py\""
    );
}
