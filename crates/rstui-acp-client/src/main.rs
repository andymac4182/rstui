//! `rstui-acp-client` — the full-screen ACP chat client binary.
//!
//! ```text
//! rstui-acp-client                       # open the registry agent picker
//! rstui-acp-client --agent "npx -y @zed-industries/claude-code-acp@latest"
//! rstui-acp-client --cmd "python my_acp_server.py"  # any custom local-stdio ACP command
//! rstui-acp-client --profile mydev       # a named recipe (command + plugins)
//! RSTUI_ACP_AGENT="./target/debug/my-acp" rstui-acp-client  # …or via the env var
//! rstui-acp-client --plugin ./my-plugin  # attach an extra plugin (repeatable)
//! ```
//!
//! `--agent`, `--cmd` and `--command` are synonyms: the **custom ACP
//! command** to launch and speak to over local stdio. `--profile <name>`
//! runs a named `command`+`plugin` recipe from
//! `~/.config/rstui/acp-client.agents`. Without any of these (and without
//! `RSTUI_ACP_AGENT`) the registry picker opens — which also offers a
//! "Custom command…" entry. With no `--plugin`, the reference plugins
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
         USAGE:\n  rstui-acp-client [--profile <name> | --cmd <acp-command>] [--plugin <cmd>]…\n\n\
         OPTIONS:\n\
         \x20 --profile <name> Run a named recipe (command + plugins) from\n\
         \x20                  ~/.config/rstui/acp-client.agents\n\
         \x20 --cmd <cmd>      Custom ACP command to run & speak to over local\n\
         \x20                  stdio (skip the picker). --agent and --command\n\
         \x20                  are synonyms.\n\
         \x20 --plugin <cmd>   Attach an extension plugin process (repeatable)\n\
         \x20 -h, --help       Show this help\n\n\
         ENV:\n\
         \x20 RSTUI_ACP_AGENT          Custom ACP command when no --cmd/--profile\n\
         \x20 RSTUI_ACP_AGENTS_FILE    Override the profiles file path\n\
         \x20 RSTUI_ACP_CONNECT_TIMEOUT Seconds to wait for the ACP handshake\n\
         \x20                          before giving up (default 30)\n\n\
         The command is split with shell-style quoting, so spaced paths and\n\
         quoted args work: --cmd 'python \"/p with space/s.py\" --flag'.\n\
         It must speak ACP (JSON-RPC 2.0) over stdio — anything else on its\n\
         stdout hangs the handshake (you get a timeout error, not a freeze).\n\n\
         Precedence: --cmd › --profile › RSTUI_ACP_AGENT › the registry\n\
         picker (which also has a \"Custom command…\" entry). With no --plugin\n\
         the in-tree reference plugins beside this binary are auto-attached.\n\n\
         Profiles file (INI; [name] sections, `command =`, repeatable\n\
         `plugin =`). Via cargo (note the `--` before the switch):\n\
         \x20 cargo run -p rstui-acp-client -- --profile mydev"
    );
}
