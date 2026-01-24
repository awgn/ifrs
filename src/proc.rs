use anyhow::Result;
use smol_str::SmolStr;
use std::collections::HashSet;

#[cfg(target_os = "macos")]
use crate::macos;

// This struct will hold the interface name and its network namespace, if any.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LinuxNic {
    pub name: SmolStr,
    pub netns: Option<SmolStr>,
}

#[cfg(target_os = "linux")]
pub fn get_if_list() -> Result<Vec<LinuxNic>> {
    use nix::sched::{setns, CloneFlags};
    use std::fs;

    use std::os::unix::fs::MetadataExt; // for ino()
    use std::path::Path;

    // Function to check if we are in the root network namespace
    fn is_root_netns() -> bool {
        let Ok(self_meta) = fs::metadata("/proc/self/ns/net") else {
            return false;
        };
        let Ok(init_meta) = fs::metadata("/proc/1/ns/net") else {
            return false;
        };
        self_meta.ino() == init_meta.ino()
    }

    let mut nics = Vec::new();
    let mut seen_nics = HashSet::new();

    // If we are not in the root netns, or we are not euid 0, just list local interfaces.
    if !is_root_netns() || !nix::unistd::geteuid().is_root() {
        for ifa in nix::ifaddrs::getifaddrs()? {
            let nic = LinuxNic {
                name: SmolStr::from(ifa.interface_name),
                netns: None, // No specific namespace context from this perspective
            };
            if seen_nics.insert(nic.clone()) {
                nics.push(nic);
            }
        }
        nics.sort_by(|a, b| a.name.cmp(&b.name));
        return Ok(nics);
    }

    // We are in the root netns and we are root. Let's scan all namespaces.

    // 1. Get interfaces from the root namespace
    for ifa in nix::ifaddrs::getifaddrs()? {
        let nic = LinuxNic {
            name: SmolStr::from(ifa.interface_name),
            netns: None,
        };
        if seen_nics.insert(nic.clone()) {
            nics.push(nic);
        }
    }

    // 2. Scan and switch to other namespaces
    let ns_dir = Path::new("/var/run/netns");
    if ns_dir.exists() {
        let original_ns = fs::File::open("/proc/self/ns/net")?;
        for entry in fs::read_dir(ns_dir)?.flatten() {
            let ns_name_os = entry.file_name();
            let ns_name = ns_name_os.to_string_lossy();
            if let Ok(ns_file) = fs::File::open(entry.path()) {
                // Switch, get interfaces, switch back
                if setns(ns_file, CloneFlags::CLONE_NEWNET).is_ok() {
                    if let Ok(ifaddrs) = nix::ifaddrs::getifaddrs() {
                        for ifa in ifaddrs {
                            let nic = LinuxNic {
                                name: SmolStr::from(ifa.interface_name),
                                netns: Some(SmolStr::from(ns_name.as_ref())),
                            };
                            if seen_nics.insert(nic.clone()) {
                                nics.push(nic);
                            }
                        }
                    }
                    // Switch back to original ns
                    let _ = setns(&original_ns, CloneFlags::CLONE_NEWNET);
                }
            }
        }
    }

    nics.sort_by_key(|k| k.name.clone());
    Ok(nics)
}

#[cfg(not(target_os = "linux"))]
pub fn get_if_list() -> Result<Vec<LinuxNic>> {
    let addrs = nix::ifaddrs::getifaddrs()?;
    let mut names = HashSet::new();
    for ifa in addrs {
        names.insert(ifa.interface_name);
    }
    let mut ret: Vec<SmolStr> = names.into_iter().map(SmolStr::from).collect();
    ret.sort();
    Ok(ret
        .into_iter()
        .map(|name| LinuxNic {
            name: SmolStr::from(name),
            netns: None,
        })
        .collect())
}

#[derive(Default, Debug)]
pub struct Stats {
    pub rx_bytes: u64,
    pub rx_packets: u64,

    pub tx_bytes: u64,
    pub tx_packets: u64,
}

#[cfg(target_os = "linux")]
pub fn get_stats(ifname: &str) -> Result<Stats> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    use std::path::Path;

    let path = Path::new("/proc/net/dev");
    if !path.exists() {
        return Ok(Stats::default());
    }
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    for (i, line) in reader.lines().enumerate() {
        if i < 2 {
            continue;
        }
        let line = line?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let name = parts[0].trim_end_matches(':');
        if name == ifname {
            if parts.len() < 11 {
                break;
            } // Safety

            let p = |idx: usize| parts[idx].parse::<u64>().unwrap_or(0);

            return Ok(Stats {
                rx_bytes: p(1),
                rx_packets: p(2),
                tx_bytes: p(9),
                tx_packets: p(10),
            });
        }
    }
    Ok(Stats::default())
}

#[cfg(target_os = "macos")]
pub fn get_stats(ifname: &str) -> Result<Stats> {
    Ok(macos::get_stats(ifname).unwrap_or_default())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn get_stats(_ifname: &str) -> Result<Stats> {
    Ok(Stats::default())
}

pub fn get_inet6_addr(ifname: &str) -> Result<Vec<(SmolStr, u32, SmolStr)>> {
    let addrs = nix::ifaddrs::getifaddrs()?;
    let mut ret = Vec::new();
    for ifa in addrs {
        if ifa.interface_name == ifname {
            if let Some(address) = ifa.address {
                if let Some(sockaddr) = address.as_sockaddr_in6() {
                    let ip = sockaddr.ip();
                    // Calculate prefix len from netmask if available
                    let mut prefix = 0;
                    if let Some(netmask) = ifa.netmask.as_ref().and_then(|a| a.as_sockaddr_in6()) {
                        let mask_ip = netmask.ip();
                        prefix = u128::from(mask_ip).count_ones();
                    }
                    // Simplified scope detection
                    let scope = if ip.is_loopback() {
                        "host"
                    } else if ip.is_unicast_link_local() {
                        "link"
                    } else if ip.octets()[0] == 0xfe && (ip.octets()[1] & 0xc0) == 0xc0 {
                        "site"
                    } else if ip.is_multicast() {
                        "multicast"
                    } else {
                        "global"
                    };

                    ret.push((SmolStr::from(ip.to_string()), prefix, SmolStr::from(scope)));
                }
            }
        }
    }
    Ok(ret)
}
