//! Standalone Telegram MTProto bridge for the WinForms port of Z GUI.
//!
//! The Tauri build ran the same accept loop in-process (`src/telegram.rs`);
//! the WinForms build runs it as a hidden child process that reports the
//! bound socket to the GUI through stdout and keeps a stats file fresh.
//!
//! stdout carries only machine-readable `ZGUI_*` lines — the crate's own
//! tracing output goes to stderr.

use std::path::PathBuf;
use std::time::Duration;

use tg_ws_proxy_rs::config::Config;
use tg_ws_proxy_rs::server::{self, ListenInfo};
use tg_ws_proxy_rs::stats::STATS;

/// How often the stats file is rewritten for the GUI.
const STATS_INTERVAL: Duration = Duration::from_secs(15);

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let (args, stats_file) = split_args();
    let mut full: Vec<String> = vec!["zgui-bridge".into()];
    full.extend(args);

    let config = match Config::try_from_args(full) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ZGUI_CONFIG_ERR\t{e}");
            std::process::exit(2);
        }
    };

    let stats = tokio::spawn(stats_loop(stats_file));

    // The GUI stops the bridge by killing the process tree (no console is
    // attached, so there is nothing to send Ctrl-C to): the shutdown future
    // simply never fires and the process serves until killed.
    let shutdown = std::future::pending::<()>();
    let result = server::run_with_listen(config, shutdown, print_listen).await;
    stats.abort();

    if let Err(e) = result {
        eprintln!("ZGUI_RUN_ERR\t{e}");
        std::process::exit(1);
    }
}

fn print_listen(info: ListenInfo) {
    println!("ZGUI_LISTEN\t{}\t{}", info.addr, info.tg_link);
}

/// Splits the bridge-only `--stats-file PATH` out of the crate's CLI args and
/// applies the same defaults `telegram.rs` used (`--default-domains`).
fn split_args() -> (Vec<String>, Option<PathBuf>) {
    let mut stats_file = None;
    let mut kept = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--stats-file" {
            stats_file = args.next().map(PathBuf::from);
            continue;
        }
        kept.push(arg);
    }
    if !kept.iter().any(|a| a == "--default-domains") {
        kept.push("--default-domains".into());
    }
    (kept, stats_file)
}

async fn stats_loop(stats_file: Option<PathBuf>) {
    let path = match stats_file {
        Some(p) => p,
        None => return,
    };
    loop {
        let _ = std::fs::write(&path, STATS.summary());
        tokio::time::sleep(STATS_INTERVAL).await;
    }
}
