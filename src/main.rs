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
    keywords: Vec<String>,
}

struct Matcher {
    keywords: Vec<String>,
    ipv4: bool,
    ipv6: bool,
    running: bool,
    ignore_case: bool,
    drivers: Vec<String>,
    all: bool,
}

impl Matcher {
    fn from_cli(cli: &Cli) -> Self {
        Self {
            keywords: cli.keywords.clone(),
            ipv4: cli.ipv4,
            ipv6: cli.ipv6,
            running: cli.running,
            ignore_case: cli.ignore_case,
            drivers: cli.driver.clone(),
            all: cli.all,
        }
    }

    fn matches(&self, info: &CollectedInterface) -> bool {
        // 1. Check -r (running)
        if self.running && !info.link_detected {
            return false;
        }

        // 2. Check -4 (ipv4)
        if self.ipv4 && info.ipv4.is_empty() {
            return false;
        }

        // 3. Check -6 (ipv6)
        if self.ipv6 && info.ipv6.is_empty() {
            return false;
        }
        
        // 4. Check -a (all) vs UP status
        // If specific keywords are provided, we might want to show them even if down, 
        // but the original logic was: if !all && !up && !explicit_list -> skip.
        // Here keywords act as explicit list if they match the name.
        let explicit_name_match = if !self.keywords.is_empty() {
             self.keywords.contains(&info.name)
        } else {
             false
        };

        if !self.all && !info.is_up && !explicit_name_match {
             return false;
        }

        // 5. Driver filter
        if !self.drivers.is_empty() {
             let mut matched = false;
             if let Some((drv, _, _)) = &info.driver_info {
                 let drv_str = drv.to_lowercase();
                 for d in &self.drivers {
                     if drv_str.contains(&d.to_lowercase()) {
                         matched = true;
                         break;
                     }
                 }
             }
             if !matched { return false; }
        }

        // 6. Keywords Matcher
        if !self.keywords.is_empty() {
             let mut any_keyword_matched = false;
             
             let targets = vec![
                 &info.name,
                 &info.flags_str,
                 &info.media,
             ];
             
             for keyword in &self.keywords {
                 if self.check_match(keyword, &targets, info) {
                     any_keyword_matched = true;
                     break;
                 }
             }
             
             if !any_keyword_matched {
                 return false;
             }
        }

        true
    }
    
    fn check_match(&self, keyword: &str, targets: &[&String], info: &CollectedInterface) -> bool {
        let k = if self.ignore_case { keyword.to_lowercase() } else { keyword.to_string() };
        
        let check = |s: &str| {
            if self.ignore_case {
                s.to_lowercase().contains(&k)
            } else {
                s.contains(&k)
            }
        };

        for t in targets {
            if check(t) { return true; }
        }
        
        if let Some(mac) = &info.mac {
            if check(mac) { return true; }
        }
        
        for (ip, _, _) in &info.ipv4 {
            if check(ip) { return true; }
        }
        
        for (ip, _, _) in &info.ipv6 {
            if check(ip) { return true; }
        }
        
        if let Some((drv, ver, bus)) = &info.driver_info {
            if check(drv) { return true; }
            if check(ver) { return true; }
            if check(bus) { return true; }
        }

        if let Some(pci) = &info.pci_info {
            if let Some(addr) = pci.pci_address() {
                if check(&addr) { return true; }
            }
            if let Some(vendor) = &pci.vendor_name {
                if check(vendor) { return true; }
            }
            if let Some(device) = &pci.device_name {
                if check(device) { return true; }
            }
        }

        false
    }
}

struct CollectedInterface {
    name: String,
    netns: Option<String>,
    is_up: bool,
    link_detected: bool,
    mac: Option<String>,
    ipv4: Vec<(String, String, i32)>, // addr, mask, prefix
    ipv6: Vec<(String, u32, String)>, // addr, prefix, scope
    flags_str: String,
    driver_info: Option<(String, String, String)>, // driver, version, bus_info
    pci_info: Option<pci_utils::PciDeviceInfo>,
    mtu: i32,
    metric: i32,
    media: String,
    stats: Option<proc::Stats>,
}

impl CollectedInterface {
    fn gather(
        nic: &proc::LinuxNic, 
        #[cfg(not(target_os = "macos"))] pci_devices: &std::collections::HashMap<String, pci_utils::PciDeviceInfo>
    ) -> Result<Self> {
        let name = &nic.name;
        let iif = ifr::Interface::new(name)?;

        let is_up = iif.is_up();
        let link_detected = iif.ethtool_link().unwrap_or(false);
        let mac = iif.mac().ok().filter(|m| !m.is_empty());
        
        let ipv4 = iif.inet_addrs();
        let ipv6 = proc::get_inet6_addr(name).unwrap_or_default();
        
        let flags_str = iif.flags_str();
        
        let drv_info_raw = iif.ethtool_drvinfo().ok();
        let driver_info = if let Some(info) = drv_info_raw {
             let drv_str = unsafe { std::ffi::CStr::from_ptr(info.driver.as_ptr()) }.to_string_lossy().to_string();
             let ver_str = unsafe { std::ffi::CStr::from_ptr(info.version.as_ptr()) }.to_string_lossy().to_string();
             let bus_str = unsafe { std::ffi::CStr::from_ptr(info.bus_info.as_ptr()) }.to_string_lossy().to_string();
             Some((drv_str, ver_str, bus_str))
        } else {
            None
        };
        
        let bus_str_owned = driver_info.as_ref().map(|(_, _, b)| b.clone()).unwrap_or_default();

        #[cfg(not(target_os = "macos"))]
        let pci_info = pci_utils::find_pci_info_for_interface(name, &bus_str_owned, pci_devices);
        #[cfg(target_os = "macos")]
        let pci_info = macos::get_pci_info_from_ioreg(name);

        let mtu = iif.mtu().unwrap_or(0);
        let metric = iif.metric().unwrap_or(0);
        
        let media = iif.media().unwrap_or_else(|_| "unknown".to_string());
        
        let stats = proc::get_stats(name).ok();

        Ok(Self {
            name: name.clone(),
            netns: nic.netns.clone(),
            is_up,
            link_detected,
            mac,
            ipv4,
            ipv6,
            flags_str,
            driver_info,
            pci_info,
            mtu,
            metric,
            media,
            stats,
        })
    }

    fn print(&self, verbose: bool) {
        if self.link_detected {
            print!("{} ", self.name.bold().bright_blue());
            print!("{}", "[link-up]".bright_black());
        } else {
            print!("{} ", self.name.blue());
            print!("{}", "[link-down]".bright_black());
        }

        if let Some(netns) = &self.netns {
            print!(" {{{}}}", netns.bright_white());
        }

        println!(); 

        let indent = "    ";

        if let Some(mac) = &self.mac {
            println!("{}MAC:     {}", indent, mac.blue());
        }

        for (addr, _mask, prefix) in &self.ipv4 {
            println!("{}IPv4:    {}/{}", indent, addr.blue(), prefix);
        }
        
        for (addr, plen, _scope) in &self.ipv6 {
             println!("{}IPv6:    {}/{}", indent, addr.blue(), plen);
        }

        if !self.flags_str.is_empty() {
             println!("{}Flags:   {}", indent, self.flags_str.dimmed());
        }

        if let Some((drv, ver, bus)) = &self.driver_info {
             println!("{}Driver:  {} (v: {})", indent, drv.blue().bold(), ver);
             if !bus.is_empty() {
                  println!("{}Bus:     {}", indent, bus);
             }
        }

        if let Some(pci_info) = &self.pci_info {
            if let Some(addr) = pci_info.pci_address() {
                println!("{}PCI:     {}", indent, addr.bright_blue());
            }

            if let (Some(vendor), Some(device)) = (&pci_info.vendor_name, &pci_info.device_name) {
                println!("{}Device:  {} {}", indent, vendor.bright_blue(), device.bright_blue());
            } else if pci_info.vendor_id != 0 || pci_info.device_id != 0 {
                println!("{}Device:  [{:04x}:{:04x}]", indent, pci_info.vendor_id, pci_info.device_id);
            }

            if verbose {
                // Verbose PCI info
            }
        }

        println!("{}MTU:     {} (Metric: {})", indent, self.mtu, self.metric);

        if self.media != "unknown" {
            println!("{}Media:   {}", indent, self.media.dimmed());
        }

        if let Some(stats) = &self.stats {
             if stats.rx_bytes > 0 || stats.tx_bytes > 0 {
                  println!("{}Stats:   RX: {} bytes ({} pkts), TX: {} bytes ({} pkts)",
                      indent,
                      stats.rx_bytes, stats.rx_packets,
                      stats.tx_bytes, stats.tx_packets
                  );
             }
        }

        println!(); 
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let matcher = Matcher::from_cli(&cli);

    #[cfg(not(target_os = "macos"))]
    let pci_devices = pci_utils::get_pci_devices().unwrap_or_default();

    let all_interfaces = proc::get_if_list()?;

    #[cfg(target_os = "linux")]
    let original_ns = fs::File::open("/proc/self/ns/net").ok();

    for nic in &all_interfaces {
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

        let result: Result<()> = (|| {
            #[cfg(not(target_os = "macos"))]
            let info = CollectedInterface::gather(nic, &pci_devices)?;
            #[cfg(target_os = "macos")]
            let info = CollectedInterface::gather(nic)?;

            if matcher.matches(&info) {
                info.print(cli.verbose);
            }
            Ok(())
        })();

        if let Err(e) = result {
             eprintln!("Error processing interface {}: {}", nic.name, e.red());
        }

        #[cfg(target_os = "linux")]
        if switched_ns {
            if let Some(ref orig_ns) = original_ns {
                let _ = setns(orig_ns, CloneFlags::CLONE_NEWNET);
            }
        }
    }

    Ok(())
}