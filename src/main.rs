use anyhow::Result;
use clap::Parser;
use cosmic_app_focus::focus;

/// Launch or focus an application by app-id / desktop-id (ex: org.mozilla.firefox or firefox)
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// App ID (Wayland app_id or desktop file ID)
    app_id: String,
    /// Command to launch if not running (default: gtk-launch <app_id>)
    #[arg(long)]
    launch_cmd: Option<String>,
    /// Increase logging verbosity (-v, -vv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

fn main() -> Result<()> {
    let args = Args::parse();
    focus::init_logger(args.verbose);
    focus::focus_or_launch(&args.app_id, args.launch_cmd.as_deref())
}
