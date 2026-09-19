// taulerbox: the CLI over the library in lib.rs. Argument parsing and the
// (not yet written) window loop live here; layout placement math lives in
// `compose`, where it can be tested without a display server.

use std::path::PathBuf;

use clap::Parser;

/// A native window that shows a tauler layout's Panels as pixel buffers,
/// alongside a microVM compartment.
#[derive(Parser)]
#[command(name = "taulerbox")]
struct Args {
    /// The layout file to evaluate (e.g. `layout.op.mdx`).
    layout: PathBuf,

    /// Bind-mounted as the compartment's home directory.
    #[arg(long)]
    home: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    anyhow::ensure!(
        args.layout.is_file(),
        "layout file not found: {}",
        args.layout.display()
    );
    anyhow::ensure!(
        args.home.is_dir(),
        "--home is not a directory: {}",
        args.home.display()
    );

    anyhow::bail!("taulerbox does not open a window yet (issue #582)");
}
