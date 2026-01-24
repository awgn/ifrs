use libc::{c_char, c_int, c_ulong, c_void};
use nix::sys::socket::{socket, AddressFamily, SockFlag, SockType};
use smol_str::SmolStr;
use std::io;
use std::mem;
use std::os::fd::{AsRawFd, OwnedFd};

#[cfg(target_os = "linux")]
use futures::stream::TryStreamExt;

// IOCTL Constants
#[cfg(target_os = "linux")]
pub const SIOCGIFHWADDR: c_ulong = 0x8927;
#[cfg(target_os = "linux")]
pub const SIOCGIFMTU: c_ulong = 0x8921;
#[cfg(target_os = "linux")]
pub const SIOCGIFMETRIC: c_ulong = 0x891d;
#[cfg(target_os = "linux")]
pub const SIOCETHTOOL: c_ulong = 0x8946;

#[cfg(target_os = "macos")]
pub const SIOCGIFMTU: c_ulong = 0xc0206933;
#[cfg(target_os = "macos")]
pub const SIOCGIFMETRIC: c_ulong = 0xc020691d;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub const SIOCGIFMTU: c_ulong = 0;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub const SIOCGIFMETRIC: c_ulong = 0;

// Ethtool Constants
#[cfg(target_os = "linux")]
pub const ETHTOOL_GDRVINFO: u32 = 0x00000003;

// If flags
pub const IFF_UP: i16 = 0x1;

// Structs

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IfMap {
    pub mem_start: c_ulong,
    pub mem_end: c_ulong,
    pub base_addr: u16,
    pub irq: u8,
    pub dma: u8,
    pub port: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union IfrData {
    pub ifru_addr: libc::sockaddr,
    pub ifru_dstaddr: libc::sockaddr,
    pub ifru_broadaddr: libc::sockaddr,
    pub ifru_netmask: libc::sockaddr,
    pub ifru_hwaddr: libc::sockaddr,
    pub ifru_flags: i16,
    pub ifru_ivalue: c_int,
    pub ifru_mtu: c_int,
    pub ifru_map: IfMap,
    pub ifru_slave: [c_char; 16],
    pub ifru_newname: [c_char; 16],
    pub ifru_data: *mut c_void,
}

#[repr(C)]
pub struct IfReq {
    pub ifr_name: [c_char; 16],
    pub ifr_ifru: IfrData,
}

impl IfReq {
    pub fn new(name: &str) -> Self {
        let mut req: IfReq = unsafe { mem::zeroed() };
        let bytes = name.as_bytes();
        let len = std::cmp::min(bytes.len(), 15);
        for (i, &byte) in bytes.iter().enumerate().take(len) {
            req.ifr_name[i] = byte as c_char;
        }
        req.ifr_name[len] = 0;
        req
    }
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct EthtoolDrvInfo {
    pub cmd: u32,
    pub driver: [c_char; 32],
    pub version: [c_char; 32],
    pub fw_version: [c_char; 32],
    pub bus_info: [c_char; 32],
    pub erom_version: [c_char; 32],
    pub reserved2: [c_char; 12],
    pub n_priv_flags: u32,
    pub n_stats: u32,
    pub testinfo_len: u32,
    pub eedump_len: u32,
    pub regdump_len: u32,
}

// IOCTL Functions

#[cfg(target_os = "linux")]
nix::ioctl_write_ptr_bad!(ioctl_ethtool, SIOCETHTOOL, IfReq);
#[cfg(target_os = "linux")]
nix::ioctl_read_bad!(ioctl_get_hwaddr, SIOCGIFHWADDR, IfReq);
nix::ioctl_read_bad!(ioctl_get_mtu, SIOCGIFMTU, IfReq);
nix::ioctl_read_bad!(ioctl_get_metric, SIOCGIFMETRIC, IfReq);

pub struct Interface {
    name: SmolStr,
    sock: OwnedFd,
}

impl Interface {
    pub fn new(name: &str) -> io::Result<Self> {
        // Create a dummy socket for ioctls
        let sock = socket(
            AddressFamily::Inet,
            SockType::Datagram,
            SockFlag::empty(),
            None,
        )
        .map_err(io::Error::other)?;

        Ok(Self {
            name: SmolStr::from(name),
            sock,
        })
    }

    pub fn flags(&self) -> io::Result<i16> {
        let addrs = nix::ifaddrs::getifaddrs()?;
        for ifa in addrs {
            if ifa.interface_name == self.name {
                return Ok(ifa.flags.bits() as i16);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Interface not found",
        ))
    }

    pub fn is_up(&self) -> bool {
        self.flags().map(|f| f & IFF_UP != 0).unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        // IFF_RUNNING is 0x40
        self.flags().map(|f| f & 0x40 != 0).unwrap_or(false)
    }

    pub fn flags_str(&self) -> SmolStr {
        let flags = match self.flags() {
            Ok(f) => f as u16,
            Err(_) => return SmolStr::default(),
        };

        let mut ret = Vec::new();

        #[cfg(target_os = "macos")]
        {
            // macOS / BSD flags
            if flags & 0x1 != 0 {
                ret.push("UP");
            }
            if flags & 0x2 != 0 {
                ret.push("BROADCAST");
            }
            if flags & 0x4 != 0 {
                ret.push("DEBUG");
            }
            if flags & 0x8 != 0 {
                ret.push("LOOPBACK");
            }
            if flags & 0x10 != 0 {
                ret.push("POINTOPOINT");
            }
            if flags & 0x20 != 0 {
                ret.push("SMART");
            }
            if flags & 0x40 != 0 {
                ret.push("RUNNING");
            }
            if flags & 0x80 != 0 {
                ret.push("NOARP");
            }
            if flags & 0x100 != 0 {
                ret.push("PROMISC");
            }
            if flags & 0x200 != 0 {
                ret.push("ALLMULTI");
            }
            if flags & 0x400 != 0 {
                ret.push("OACTIVE");
            }
            if flags & 0x800 != 0 {
                ret.push("SIMPLEX");
            }
            if flags & 0x1000 != 0 {
                ret.push("LINK0");
            }
            if flags & 0x2000 != 0 {
                ret.push("LINK1");
            }
            if flags & 0x4000 != 0 {
                ret.push("LINK2");
            }
            if flags & 0x8000 != 0 {
                ret.push("MULTICAST");
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            // Standard Linux-like flags
            if flags & 0x1 != 0 {
                ret.push("UP");
            }
            if flags & 0x2 != 0 {
                ret.push("BROADCAST");
            }
            if flags & 0x4 != 0 {
                ret.push("DEBUG");
            }
            if flags & 0x8 != 0 {
                ret.push("LOOPBACK");
            }
            if flags & 0x10 != 0 {
                ret.push("PTP");
            }
            if flags & 0x20 != 0 {
                ret.push("NOTRAILERS");
            }
            if flags & 0x40 != 0 {
                ret.push("RUNNING");
            }
            if flags & 0x80 != 0 {
                ret.push("NOARP");
            }
            if flags & 0x100 != 0 {
                ret.push("PROMISC");
            }
            if flags & 0x200 != 0 {
                ret.push("ALLMULTI");
            }
            if flags & 0x400 != 0 {
                ret.push("MASTER");
            }
            if flags & 0x800 != 0 {
                ret.push("SLAVE");
            }
            if flags & 0x1000 != 0 {
                ret.push("MULTICAST");
            }
            if flags & 0x2000 != 0 {
                ret.push("PORTSEL");
            }
            if flags & 0x4000 != 0 {
                ret.push("AUTOMEDIA");
            }
            if flags & 0x8000 != 0 {
                ret.push("DYNAMIC");
            }
        }

        SmolStr::from(ret.join(" "))
    }

    #[cfg(target_os = "linux")]
    pub fn mac(&self) -> io::Result<SmolStr> {
        use smol_str::format_smolstr;

        let mut req = IfReq::new(&self.name);
        unsafe { ioctl_get_hwaddr(self.sock.as_raw_fd(), &mut req) }
            .map_err(|e| io::Error::from_raw_os_error(e as i32))?;
        let addr = unsafe { req.ifr_ifru.ifru_hwaddr.sa_data };
        // sa_data is [i8; 14]. MAC is first 6.
        Ok(format_smolstr!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            addr[0],
            addr[1],
            addr[2],
            addr[3],
            addr[4],
            addr[5]
        ))
    }

    #[cfg(not(target_os = "linux"))]
    pub fn mac(&self) -> io::Result<SmolStr> {
        let addrs = nix::ifaddrs::getifaddrs()?;
        for ifa in addrs {
            if ifa.interface_name == self.name {
                if let Some(address) = ifa.address {
                    if let Some(link) = address.as_link_addr() {
                        if let Some(addr) = link.addr() {
                            use smol_str::format_smolstr;

                            let s = addr
                                .iter()
                                .map(|b| format_smolstr!("{:02x}", b))
                                .collect::<Vec<_>>()
                                .join(":");
                            return Ok(s.into());
                        }
                    }
                }
            }
        }
        Ok(SmolStr::from(""))
    }

    pub fn mtu(&self) -> io::Result<i32> {
        let mut req = IfReq::new(&self.name);
        unsafe { ioctl_get_mtu(self.sock.as_raw_fd(), &mut req) }
            .map_err(|e| io::Error::from_raw_os_error(e as i32))?;
        unsafe { Ok(req.ifr_ifru.ifru_mtu) }
    }

    pub fn metric(&self) -> io::Result<i32> {
        let mut req = IfReq::new(&self.name);
        match unsafe { ioctl_get_metric(self.sock.as_raw_fd(), &mut req) } {
            Ok(_) => unsafe {
                Ok(if req.ifr_ifru.ifru_ivalue == 0 {
                    1
                } else {
                    req.ifr_ifru.ifru_ivalue
                })
            },
            Err(e) => Err(io::Error::from_raw_os_error(e as i32)),
        }
    }

    // Inet addrs (using nix::ifaddrs is easier here, as C++ uses getifaddrs)
    pub fn inet_addrs(&self) -> Vec<(SmolStr, SmolStr, i32)> {
        let mut ret = Vec::new();
        if let Ok(addrs) = nix::ifaddrs::getifaddrs() {
            for ifa in addrs {
                if ifa.interface_name == self.name {
                    if let Some(address) = ifa.address {
                        if let Some(sockaddr) = address.as_sockaddr_in() {
                            let ip_u32 = sockaddr.ip();
                            let ip = std::net::Ipv4Addr::from(ip_u32);

                            let mask_opt = ifa
                                .netmask
                                .as_ref()
                                .and_then(|a| a.as_sockaddr_in().map(|s| s.ip()));

                            if let Some(mask_u32) = mask_opt {
                                let mask_ip = std::net::Ipv4Addr::from(mask_u32);
                                // Mask is u32.
                                let prefix = u32::from(mask_ip).count_ones() as i32;

                                ret.push((
                                    SmolStr::from(ip.to_string()),
                                    SmolStr::from(mask_ip.to_string()),
                                    prefix,
                                ));
                            } else {
                                ret.push((
                                    SmolStr::from(ip.to_string()),
                                    SmolStr::new_static(""),
                                    0,
                                ));
                            }
                        }
                    }
                }
            }
        }
        ret
    }

    /// Get link status using ethtool
    #[cfg(target_os = "linux")]
    pub fn ethtool_link(&self) -> io::Result<bool> {
        Ok(self.is_running())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn ethtool_link(&self) -> io::Result<bool> {
        Ok(self.is_running())
    }

    /// Get driver information using ethtool ioctl
    #[cfg(target_os = "linux")]
    pub fn ethtool_drvinfo(&self) -> io::Result<EthtoolDrvInfo> {
        let mut info: EthtoolDrvInfo = EthtoolDrvInfo {
            cmd: ETHTOOL_GDRVINFO,
            ..Default::default()
        };

        let mut req = IfReq::new(&self.name);
        req.ifr_ifru.ifru_data = &mut info as *mut _ as *mut c_void;

        unsafe { ioctl_ethtool(self.sock.as_raw_fd(), &req) }
            .map_err(|e| io::Error::from_raw_os_error(e as i32))?;
        Ok(info)
    }

    #[cfg(not(target_os = "linux"))]
    pub fn ethtool_drvinfo(&self) -> io::Result<EthtoolDrvInfo> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Not supported on this OS",
        ))
    }

    /// Get media/link information using ethtool
    #[cfg(target_os = "linux")]
    pub fn media(&self) -> io::Result<SmolStr> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .map_err(io::Error::other)?;

        rt.block_on(async {
            let (connection, mut handle, _) =
                ethtool::new_connection().map_err(io::Error::other)?;

            tokio::spawn(connection);

            let mut link_mode_handle = handle
                .link_mode()
                .get(Some(self.name.as_str()))
                .execute()
                .await;

            if let Ok(Some(msg)) = link_mode_handle.try_next().await {
                use ethtool::{EthtoolAttr, EthtoolLinkModeAttr, EthtoolLinkModeDuplex};

                let mut speed: u32 = 0;
                let mut duplex_str = "unknown";

                for nla in &msg.payload.nlas {
                    if let EthtoolAttr::LinkMode(attr) = nla {
                        match attr {
                            EthtoolLinkModeAttr::Speed(s) => speed = *s,
                            EthtoolLinkModeAttr::Duplex(d) => {
                                duplex_str = match d {
                                    EthtoolLinkModeDuplex::Full => "full",
                                    EthtoolLinkModeDuplex::Half => "half",
                                    _ => "unknown",
                                };
                            }
                            _ => {}
                        }
                    }
                }

                if speed == 0 || speed == 0xFFFF || speed == 0xFFFFFFFF {
                    return Ok(SmolStr::from("TP (unknown speed)"));
                }
                return Ok(SmolStr::from(format!("TP {}Mb/s {}", speed, duplex_str)));
            }

            Ok(SmolStr::new_static("unknown"))
        })
    }

    #[cfg(not(target_os = "linux"))]
    pub fn media(&self) -> io::Result<SmolStr> {
        Ok(SmolStr::new_static("unknown"))
    }

    /// Get ring parameters (RX/TX ring sizes)
    #[cfg(target_os = "linux")]
    pub fn ethtool_rings(&self) -> io::Result<(u32, u32)> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .map_err(io::Error::other)?;

        rt.block_on(async {
            let (connection, mut handle, _) =
                ethtool::new_connection().map_err(io::Error::other)?;

            tokio::spawn(connection);

            let mut ring_handle = handle.ring().get(Some(self.name.as_str())).execute().await;

            if let Ok(Some(msg)) = ring_handle.try_next().await {
                use ethtool::{EthtoolAttr, EthtoolRingAttr};

                let mut rx: u32 = 0;
                let mut tx: u32 = 0;

                for nla in &msg.payload.nlas {
                    if let EthtoolAttr::Ring(attr) = nla {
                        match attr {
                            EthtoolRingAttr::Rx(val) => rx = *val,
                            EthtoolRingAttr::Tx(val) => tx = *val,
                            _ => {}
                        }
                    }
                }

                Ok((rx, tx))
            } else {
                Ok((0, 0))
            }
        })
    }

    #[cfg(not(target_os = "linux"))]
    pub fn ethtool_rings(&self) -> io::Result<(u32, u32)> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Ring info not available on this OS",
        ))
    }

    /// Get channel parameters (number of RX/TX queues)
    #[cfg(target_os = "linux")]
    pub fn ethtool_channels(&self) -> io::Result<(u32, u32, u32, u32)> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .map_err(io::Error::other)?;

        rt.block_on(async {
            let (connection, mut handle, _) =
                ethtool::new_connection().map_err(io::Error::other)?;

            tokio::spawn(connection);

            let mut channel_handle = handle
                .channel()
                .get(Some(self.name.as_str()))
                .execute()
                .await;

            if let Ok(Some(msg)) = channel_handle.try_next().await {
                use ethtool::{EthtoolAttr, EthtoolChannelAttr};

                let mut rx: u32 = 0;
                let mut tx: u32 = 0;
                let mut other: u32 = 0;
                let mut combined: u32 = 0;

                for nla in &msg.payload.nlas {
                    if let EthtoolAttr::Channel(attr) = nla {
                        match attr {
                            EthtoolChannelAttr::RxCount(val) => rx = *val,
                            EthtoolChannelAttr::TxCount(val) => tx = *val,
                            EthtoolChannelAttr::OtherCount(val) => other = *val,
                            EthtoolChannelAttr::CombinedCount(val) => combined = *val,
                            _ => {}
                        }
                    }
                }

                Ok((rx, tx, other, combined))
            } else {
                Ok((0, 0, 0, 0))
            }
        })
    }

    #[cfg(not(target_os = "linux"))]
    pub fn ethtool_channels(&self) -> io::Result<(u32, u32, u32, u32)> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Channel info not available on this OS",
        ))
    }

    /// Get active features/offloads (TSO, GSO, GRO, checksumming, etc.)
    #[cfg(target_os = "linux")]
    pub fn ethtool_features(&self) -> io::Result<Vec<SmolStr>> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .map_err(io::Error::other)?;

        rt.block_on(async {
            let (connection, mut handle, _) =
                ethtool::new_connection().map_err(io::Error::other)?;

            tokio::spawn(connection);

            let mut feature_handle = handle
                .feature()
                .get(Some(self.name.as_str()))
                .execute()
                .await;

            if let Ok(Some(msg)) = feature_handle.try_next().await {
                use ethtool::{EthtoolAttr, EthtoolFeatureAttr};

                let mut features = Vec::new();

                for nla in &msg.payload.nlas {
                    if let EthtoolAttr::Feature(EthtoolFeatureAttr::Active(bits)) = nla {
                        for bit in bits {
                            if bit.value {
                                let feature_name = match bit.name.as_str() {
                                    "tx-tcp-segmentation" => "tso",
                                    "tx-generic-segmentation" => "gso",
                                    "rx-gro" => "gro",
                                    "rx-lro" => "lro",
                                    "rx-checksum" => "rx-csum",
                                    "tx-checksum-ip-generic" => "tx-csum",
                                    "tx-checksum-ipv4" => "tx-csum-ipv4",
                                    "tx-checksum-ipv6" => "tx-csum-ipv6",
                                    "tx-scatter-gather" => "sg",
                                    "tx-scatter-gather-fraglist" => "sg-frag",
                                    "tx-vlan-hw-insert" => "tx-vlan",
                                    "rx-vlan-hw-parse" => "rx-vlan",
                                    "highdma" => "highdma",
                                    "rx-hashing" => "rxhash",
                                    "rx-ntuple-filter" => "ntuple",
                                    other => other,
                                };
                                features.push(SmolStr::from(feature_name));
                            }
                        }
                    }
                }

                Ok(features)
            } else {
                Ok(Vec::new())
            }
        })
    }

    #[cfg(not(target_os = "linux"))]
    pub fn ethtool_features(&self) -> io::Result<Vec<SmolStr>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Feature info not available on this OS",
        ))
    }
}
