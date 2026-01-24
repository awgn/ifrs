use crate::ifr;
#[cfg(target_os = "macos")]
use crate::macos;
use crate::pci_utils;
use crate::proc;
use anyhow::Result;
use owo_colors::OwoColorize;
use smol_str::SmolStr;

#[cfg(target_os = "linux")]
fn get_altname(interface_name: &str) -> Option<SmolStr> {
    use futures::stream::TryStreamExt;
    use rtnetlink::new_connection;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;

    rt.block_on(async {
        let (connection, handle, _) = new_connection().ok()?;
        tokio::spawn(connection);

        let mut links = handle
            .link()
            .get()
            .match_name(interface_name.to_string())
            .execute();
        if let Some(link) = links.try_next().await.ok()? {
            for nla in &link.nlas {
                if let rtnetlink::packet::link::nlas::Nla::PropList(prop_list) = nla {
                    for prop in prop_list {
                        if let rtnetlink::packet::link::nlas::Prop::AltIfName(altname) = prop {
                            if !altname.is_empty() {
                                return Some(SmolStr::from(altname.as_str()));
                            }
                        }
                    }
                }
            }
        }
        None
    })
}

#[cfg(not(target_os = "linux"))]
fn get_altname(_interface_name: &str) -> Option<SmolStr> {
    None
}

pub struct Matcher {
    pub keywords: Vec<SmolStr>,
    pub ipv4: bool,
    pub ipv6: bool,
    pub running: bool,
    pub ignore_case: bool,
    pub all: bool,
}

impl Matcher {
    pub fn matches(&self, info: &CollectedInterface) -> bool {
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
        let explicit_name_match = if !self.keywords.is_empty() {
            self.keywords
                .iter()
                .any(|k| k.as_str() == info.name.as_str())
        } else {
            false
        };

        if !self.all && !info.is_up && !explicit_name_match {
            return false;
        }

        // 5. Keywords Matcher
        if !self.keywords.is_empty() {
            let mut any_keyword_matched = false;

            let targets = vec![
                info.name.as_str(),
                info.flags_str.as_str(),
                info.media.as_str(),
            ];

            for keyword in &self.keywords {
                if self.check_match(keyword.as_str(), &targets, info) {
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

    fn check_match(&self, keyword: &str, targets: &[&str], info: &CollectedInterface) -> bool {
        let k = if self.ignore_case {
            keyword.to_lowercase()
        } else {
            keyword.to_string()
        };

        let check = |s: &str| {
            if self.ignore_case {
                s.to_lowercase().contains(&k)
            } else {
                s.contains(&k)
            }
        };

        for t in targets {
            if check(t) {
                return true;
            }
        }

        if let Some(mac) = &info.mac {
            if check(mac.as_str()) {
                return true;
            }
        }

        for (ip, _, _) in &info.ipv4 {
            if check(ip.as_str()) {
                return true;
            }
        }

        for (ip, _, _) in &info.ipv6 {
            if check(ip.as_str()) {
                return true;
            }
        }

        if let Some((drv, ver, bus)) = &info.driver_info {
            if check(drv.as_str()) {
                return true;
            }
            if check(ver.as_str()) {
                return true;
            }
            if check(bus.as_str()) {
                return true;
            }
        }

        if let Some(pci) = &info.pci_info {
            if let Some(addr) = pci.pci_address() {
                if check(&addr) {
                    return true;
                }
            }
            if let Some(vendor) = &pci.vendor_name {
                if check(vendor) {
                    return true;
                }
            }
            if let Some(device) = &pci.device_name {
                if check(device) {
                    return true;
                }
            }
        }

        false
    }
}

pub struct CollectedInterface {
    pub name: SmolStr,
    pub netns: Option<SmolStr>,
    pub is_up: bool,
    pub link_detected: bool,
    pub mac: Option<SmolStr>,
    pub ipv4: Vec<(SmolStr, SmolStr, i32)>, // addr, mask, prefix
    pub ipv6: Vec<(SmolStr, u32, SmolStr)>, // addr, prefix, scope
    pub flags_str: SmolStr,
    pub driver_info: Option<(SmolStr, SmolStr, SmolStr)>, // driver, version, bus_info
    pub pci_info: Option<pci_utils::PciDeviceInfo>,
    pub altname: Option<SmolStr>,
    pub mtu: i32,
    pub metric: i32,
    pub media: SmolStr,
    pub stats: Option<proc::Stats>,
    #[cfg(target_os = "linux")]
    pub rings: Option<(u32, u32)>, // rx, tx
    #[cfg(target_os = "linux")]
    pub channels: Option<(u32, u32, u32, u32)>, // rx, tx, other, combined
    #[cfg(target_os = "linux")]
    pub features: Vec<SmolStr>, // active offload features
}

impl CollectedInterface {
    pub fn gather(
        nic: &proc::LinuxNic,
        #[cfg(not(target_os = "macos"))] pci_devices: &std::collections::HashMap<
            SmolStr,
            pci_utils::PciDeviceInfo,
        >,
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
            let drv_str =
                unsafe { std::ffi::CStr::from_ptr(info.driver.as_ptr()) }.to_string_lossy();
            let ver_str =
                unsafe { std::ffi::CStr::from_ptr(info.version.as_ptr()) }.to_string_lossy();
            let bus_str =
                unsafe { std::ffi::CStr::from_ptr(info.bus_info.as_ptr()) }.to_string_lossy();
            Some((
                SmolStr::from(drv_str),
                SmolStr::from(ver_str),
                SmolStr::from(bus_str),
            ))
        } else {
            #[cfg(target_os = "macos")]
            {
                macos::get_driver_info(name).map(|(drv, ver, bus)| {
                    (SmolStr::from(drv), SmolStr::from(ver), SmolStr::from(bus))
                })
            }
            #[cfg(not(target_os = "macos"))]
            {
                None
            }
        };

        let bus_str_owned = driver_info
            .as_ref()
            .map(|(_, _, b)| b.as_str())
            .unwrap_or_default();

        #[cfg(not(target_os = "macos"))]
        let pci_info = pci_utils::find_pci_info_for_interface(name, bus_str_owned, pci_devices);
        #[cfg(target_os = "macos")]
        let _ = bus_str_owned;
        #[cfg(target_os = "macos")]
        let pci_info = macos::get_pci_info_from_ioreg(name);

        let mtu = iif.mtu().unwrap_or(0);
        let metric = iif.metric().unwrap_or(0);

        let media = iif
            .media()
            .unwrap_or_else(|_| SmolStr::new_static("unknown"));

        let stats = proc::get_stats(name).ok();

        #[cfg(target_os = "linux")]
        let rings = iif.ethtool_rings().ok();
        #[cfg(target_os = "linux")]
        let channels = iif.ethtool_channels().ok();
        #[cfg(target_os = "linux")]
        let features = iif.ethtool_features().unwrap_or_default();

        let altname = get_altname(name);

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
            altname,
            mtu,
            metric,
            media,
            stats,
            #[cfg(target_os = "linux")]
            rings,
            #[cfg(target_os = "linux")]
            channels,
            #[cfg(target_os = "linux")]
            features,
        })
    }

    pub fn print(&self, verbose: bool) {
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

        let indent = "  ";

        if let Some(mac) = &self.mac {
            println!("{}MAC:      {}", indent, mac.blue());
        }

        for (addr, _mask, prefix) in &self.ipv4 {
            println!("{}IPv4:     {}/{}", indent, addr.blue(), prefix);
        }

        for (addr, plen, _scope) in &self.ipv6 {
            println!("{}IPv6:     {}/{}", indent, addr.blue(), plen);
        }

        if !self.flags_str.is_empty() {
            println!("{}Flags:    {}", indent, self.flags_str.dimmed());
        }

        if let Some((drv, ver, bus)) = &self.driver_info {
            println!("{}Driver:   {} (v: {})", indent, drv.blue().bold(), ver);
            if !bus.is_empty() {
                println!("{}Bus:      {}", indent, bus);
            }
        }

        if let Some(altname) = &self.altname {
            println!("{}Altname:  {}", indent, altname.blue());
        }

        if let Some(pci_info) = &self.pci_info {
            if let Some(addr) = pci_info.pci_address() {
                println!("{}PCI:      {}", indent, addr.bright_blue());
            }

            if let (Some(vendor), Some(device)) = (&pci_info.vendor_name, &pci_info.device_name) {
                println!(
                    "{}Device:   {} {}",
                    indent,
                    vendor.bright_blue(),
                    device.bright_blue()
                );
            } else if pci_info.vendor_id != 0 || pci_info.device_id != 0 {
                println!(
                    "{}Device:   [{:04x}:{:04x}]",
                    indent, pci_info.vendor_id, pci_info.device_id
                );
            }

            if verbose {
                // Verbose PCI info
            }
        }

        println!("{}MTU:      {} (Metric: {})", indent, self.mtu, self.metric);

        if self.media != "unknown" {
            println!("{}Media:    {}", indent, self.media.dimmed());
        }

        #[cfg(target_os = "linux")]
        if verbose {
            if !self.features.is_empty() {
                println!("{}Features: {}", indent, self.features.join(" "));
            }
            if let Some((rx, tx)) = self.rings {
                if rx > 0 || tx > 0 {
                    println!("{}Rings:    RX: {}, TX: {}", indent, rx, tx);
                }
            }
            if let Some((rx, tx, other, combined)) = self.channels {
                if rx > 0 || tx > 0 || other > 0 || combined > 0 {
                    println!(
                        "{}Channels: RX: {}, TX: {}, Other: {}, Combined: {}",
                        indent, rx, tx, other, combined
                    );
                }
            }
        }

        if let Some(stats) = &self.stats {
            if stats.rx_bytes > 0 || stats.tx_bytes > 0 {
                println!(
                    "{}Stats:    RX: {} bytes ({} pkts), TX: {} bytes ({} pkts)",
                    indent, stats.rx_bytes, stats.rx_packets, stats.tx_bytes, stats.tx_packets
                );
            }
        }

        println!();
    }
}
