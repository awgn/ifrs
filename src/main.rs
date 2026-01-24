use anyhow::Result;
use clap::Parser;
#[cfg(target_os = "linux")]
use nix::sched::{setns, CloneFlags};
use owo_colors::OwoColorize;
use rayon::prelude::*;
use smol_str::SmolStr;
#[cfg(target_os = "linux")]
use std::fs;

mod filter;
mod ifr;
#[cfg(target_os = "macos")]
mod macos;
mod pci_utils;
mod proc;

use filter::{CollectedInterface, Matcher};

#[derive(Parser)]
#[command(
    name = "ifrs",
    about = "Show network interface information",
    version,
    author
)]
struct Cli {
    /// Display all interfaces (even if down)
    #[arg(short, long)]
    all: bool,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Show only interfaces with IPv4
    #[arg(short = '4', long = "ipv4")]
    ipv4: bool,

    /// Show only interfaces with IPv6
    #[arg(short = '6', long = "ipv6")]
    ipv6: bool,

    /// Show only running interfaces
    #[arg(short = 'r', long = "running")]
    running: bool,

    /// Case insensitive matching
    #[arg(short = 'i', long = "ignore-case")]
    ignore_case: bool,

    /// Interface list / Keywords
    #[arg(trailing_var_arg = true)]
    keywords: Vec<SmolStr>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let matcher = Matcher {
        keywords: cli.keywords.into_iter().collect(),
        ipv4: cli.ipv4,
        ipv6: cli.ipv6,
        running: cli.running,
        ignore_case: cli.ignore_case,
        all: cli.all,
    };

    #[cfg(not(target_os = "macos"))]
    let pci_devices = pci_utils::get_pci_devices().unwrap_or_default();

    let all_interfaces = proc::get_if_list()?;

    #[cfg(target_os = "linux")]
    let original_ns = fs::File::open("/proc/self/ns/net").ok();

    let results: Vec<_> = all_interfaces
        .par_iter()
        .map(|nic| {
            // --- Namespace Switching (Linux specific) ---
            #[cfg(target_os = "linux")]
            let mut switched_ns = false;
            #[cfg(target_os = "linux")]
            if let (Some(ns_name), Some(_)) = (&nic.netns, &original_ns) {
                if let Ok(ns_file) = fs::File::open(format!("/var/run/netns/{}", ns_name)) {
                    if setns(ns_file, CloneFlags::CLONE_NEWNET).is_ok() {
                        switched_ns = true;
                    }
                }
            }

            let result: Result<CollectedInterface> = (|| {
                #[cfg(not(target_os = "macos"))]
                let info = CollectedInterface::gather(nic, &pci_devices)?;
                #[cfg(target_os = "macos")]
                let info = CollectedInterface::gather(nic)?;
                Ok(info)
            })();

            #[cfg(target_os = "linux")]
            if switched_ns {
                if let Some(ref orig_ns) = original_ns {
                    let _ = setns(orig_ns, CloneFlags::CLONE_NEWNET);
                }
            }

            (nic, result)
        })
        .collect();

    for (nic, result) in results {
        match result {
            Ok(info) => {
                if matcher.matches(&info) {
                    info.print(cli.verbose);
                }
            }
            Err(e) => {
                eprintln!("Error processing interface {}: {}", nic.name, e.red());
            }
        }
    }

    Ok(())
}
