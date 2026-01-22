use clap::Parser;
use owo_colors::OwoColorize;
use anyhow::Result;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use nix::sched::{setns, CloneFlags};

mod ifr;
mod proc;
mod pci_utils;
#[cfg(target_os = "macos")]
mod macos;

#[derive(Parser)]
#[command(name = "ifshow", about = "Show network interface information")]
struct Cli {
    /// Display all interfaces (even if down)
    #[arg(short, long)]
    all: bool,

    /// Filter by driver
    #[arg(short, long)]
    driver: Vec<String>,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Interface list
    #[arg(trailing_var_arg = true)]
    interfaces: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Enumerate PCI devices once, this is namespace-agnostic
    #[cfg(not(target_os = "macos"))]
    let pci_devices = pci_utils::get_pci_devices().unwrap_or_default();

    let all_interfaces = proc::get_if_list()?;

    #[cfg(target_os = "linux")]
    let original_ns = fs::File::open("/proc/self/ns/net").ok();

    for nic in &all_interfaces {
        let name = &nic.name;
        // Filter by name
        if !cli.interfaces.is_empty() && !cli.interfaces.iter().any(|i| name.contains(i)) {
            continue;
        }

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

        // All data gathering for an interface happens within its namespace
        let result: Result<()> = (|| {
            let iif = match ifr::Interface::new(name) {
                Ok(i) => i,
                Err(_) => return Ok(()),
            };

            // Filter by flags (UP) unless -a
            let is_up = iif.is_up();
            if !cli.all && !is_up && !cli.interfaces.contains(name) {
                 return Ok(());
            }

            // --- Driver Info (Linux) ---
            let drv_info = iif.ethtool_drvinfo().ok();

            // Filter by driver (if filter active)
            if !cli.driver.is_empty() {
                 let mut matched = false;
                 if let Some(info) = &drv_info {
                     let drv_str = unsafe { std::ffi::CStr::from_ptr(info.driver.as_ptr()) }.to_string_lossy().to_lowercase();
                     for d in &cli.driver {
                         if drv_str.to_lowercase().contains(&d.to_lowercase()) {
                             matched = true;
                             break;
                         }
                     }
                 }
                 if !matched { return Ok(()); }
            }

            let link_detected = iif.ethtool_link().unwrap_or(false);

            if link_detected {
                print!("{} ", name.bold().bright_blue());
                print!("{}", "[link-up]".bright_black());
            } else {
                print!("{} ", name.blue());
                print!("{}", "[link-down]".bright_black());
            }

            // --- Header (Name + Namespace + Link Status) ---
            if let Some(netns) = &nic.netns {
                print!(" {{{}}}", netns.bright_white());
            }

            println!(); // End of header line

            let indent = "    ";

            // --- Hardware Address (MAC) ---
            if let Ok(mac) = iif.mac() {
                if !mac.is_empty() {
                    println!("{}MAC:     {}", indent, mac.blue());
                }
            }

            // --- IP Addresses ---
            // IPv4
            for (addr, _mask, prefix) in iif.inet_addrs() {
                println!("{}IPv4:    {}/{}", indent, addr.blue(), prefix);
            }
            // IPv6
            if let Ok(addrs) = proc::get_inet6_addr(name) {
                 for (addr, plen, _scope) in addrs {
                      println!("{}IPv6:    {}/{}", indent, addr.blue(), plen);
                 }
            }

            // --- Status / Flags ---
            let flags_str = iif.flags_str();
            if !flags_str.is_empty() {
                 println!("{}Flags:   {}", indent, flags_str.dimmed());
            }

            // --- Driver / Bus Info ---
            #[allow(unused_variables)]
            let bus_str_owned = if let Some(info) = &drv_info {
                 let drv_str = unsafe { std::ffi::CStr::from_ptr(info.driver.as_ptr()) }.to_string_lossy();
                 let ver_str = unsafe { std::ffi::CStr::from_ptr(info.version.as_ptr()) }.to_string_lossy();
                 let bus_str = unsafe { std::ffi::CStr::from_ptr(info.bus_info.as_ptr()) }.to_string_lossy();
                 let s = bus_str.to_string();

                 println!("{}Driver:  {} (v: {})", indent, drv_str.blue().bold(), ver_str);
                 if !s.is_empty() {
                      println!("{}Bus:     {}", indent, s);
                 }
                 s
            } else {
                 String::new()
            };

            // --- PCI Info (namespace agnostic) ---
            #[cfg(not(target_os = "macos"))]
            let pci_info_opt = pci_utils::find_pci_info_for_interface(name, &bus_str_owned, &pci_devices);
            #[cfg(target_os = "macos")]
            let pci_info_opt = macos::get_pci_info_from_ioreg(name);

            if let Some(pci_info) = pci_info_opt {
                if let Some(addr) = pci_info.pci_address() {
                    println!("{}PCI:     {}", indent, addr.bright_blue());
                }

                if let (Some(vendor), Some(device)) = (&pci_info.vendor_name, &pci_info.device_name) {
                    println!("{}Device:  {} {}", indent, vendor.bright_blue(), device.bright_blue());
                } else if pci_info.vendor_id != 0 || pci_info.device_id != 0 {
                    println!("{}Device:  [{:04x}:{:04x}]", indent, pci_info.vendor_id, pci_info.device_id);
                }

                if cli.verbose {
                    // Verbose PCI info
                }
            }


            // --- MTU / Metric ---
            let mtu = iif.mtu().unwrap_or(0);
            let metric = iif.metric().unwrap_or(0);
            println!("{}MTU:     {} (Metric: {})", indent, mtu, metric);

            // --- Media ---
            if let Ok(media) = iif.media() {
                if media != "unknown" {
                    println!("{}Media:   {}", indent, media.dimmed());
                }
            }

            // --- Stats ---
            let stats_opt = proc::get_stats(name).ok();

            if let Some(stats) = stats_opt {
                 if stats.rx_bytes > 0 || stats.tx_bytes > 0 {
                      println!("{}Stats:   RX: {} bytes ({} pkts), TX: {} bytes ({} pkts)",
                          indent,
                          stats.rx_bytes, stats.rx_packets,
                          stats.tx_bytes, stats.tx_packets
                      );
                 }
            }

            println!(); // Separator
            Ok(())
        })();

        if let Err(e) = result {
             eprintln!("Error processing interface {}: {}", name, e.red());
        }

        // --- Switch back to original namespace ---
        #[cfg(target_os = "linux")]
        if switched_ns {
            if let Some(ref orig_ns) = original_ns {
                let _ = setns(orig_ns, CloneFlags::CLONE_NEWNET);
            }
        }
    }

    Ok(())
}
