use std::io;
use smol_str::SmolStr;
use std::mem;
use std::os::fd::{AsRawFd, OwnedFd};
use nix::sys::socket::{socket, AddressFamily, SockFlag, SockType};
use libc::{c_char, c_int, c_ulong, c_void};

#[cfg(target_os = "macos")]
use crate::macos;

// IOCTL Constants
#[cfg(target_os = "linux")]
pub const SIOCGIFHWADDR: c_ulong = 0x8927;
#[cfg(target_os = "linux")]
pub const SIOCGIFMTU: c_ulong = 0x8921;
#[cfg(target_os = "linux")]
pub const SIOCGIFMETRIC: c_ulong = 0x891d;
#[cfg(target_os = "linux")]
pub const SIOCGIFMAP: c_ulong = 0x8970;
#[cfg(target_os = "linux")]
pub const SIOCGIFTXQLEN: c_ulong = 0x8942;
#[cfg(target_os = "linux")]
pub const SIOCETHTOOL: c_ulong = 0x8946;

#[cfg(target_os = "macos")]
pub const SIOCGIFMTU: c_ulong = 0xc0206933;
#[cfg(target_os = "macos")]
pub const SIOCGIFMETRIC: c_ulong = 0xc020691d;
#[cfg(target_os = "macos")]
pub const SIOCGIFMEDIA: c_ulong = 0xc030693e;





#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub const SIOCGIFMTU: c_ulong = 0;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub const SIOCGIFMETRIC: c_ulong = 0;

// Ethtool Constants
#[cfg(target_os = "linux")]
pub const ETHTOOL_GDRVINFO: u32 = 0x00000003;
#[cfg(target_os = "linux")]
pub const ETHTOOL_GSET: u32 = 0x00000001;
#[cfg(target_os = "linux")]
pub const ETHTOOL_GLINK: u32 = 0x0000000a;

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

#[cfg(target_os = "macos")]
#[repr(C)]
pub struct IfMediaReq {
    pub ifm_name: [c_char; 16],
    pub ifm_current: c_int,
    pub ifm_mask: c_int,
    pub ifm_status: c_int,
    pub ifm_active: c_int,
    pub ifm_count: c_int,
    pub ifm_ulist: *mut c_int,
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



#[repr(C)]
#[derive(Debug, Default)]
#[cfg(target_os = "linux")]
pub struct EthtoolValue {
    pub cmd: u32,
    pub data: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
#[cfg(target_os = "linux")]
pub struct EthtoolCmd {
    pub cmd: u32,
    pub supported: u32,
    pub advertising: u32,
    pub speed: u16,
    pub duplex: u8,
    pub port: u8,
    pub phy_address: u8,
    pub transceiver: u8,
    pub autoneg: u8,
    pub mdio_support: u8,
    pub maxtxpkt: u32,
    pub maxrxpkt: u32,
    pub speed_hi: u16,
    pub eth_tp_mdix: u8,
    pub eth_tp_mdix_ctrl: u8,
    pub lp_advertising: u32,
    pub reserved: [u32; 2],
}

// IOCTL Functions

#[cfg(target_os = "linux")]
nix::ioctl_write_ptr_bad!(ioctl_ethtool, SIOCETHTOOL, IfReq);
#[cfg(target_os = "linux")]
nix::ioctl_read_bad!(ioctl_get_hwaddr, SIOCGIFHWADDR, IfReq);
nix::ioctl_read_bad!(ioctl_get_mtu, SIOCGIFMTU, IfReq);
nix::ioctl_read_bad!(ioctl_get_metric, SIOCGIFMETRIC, IfReq);
#[cfg(target_os = "macos")]
nix::ioctl_read_bad!(ioctl_get_media, SIOCGIFMEDIA, IfMediaReq);



pub struct Interface {
    name: SmolStr,
    sock: OwnedFd,
}

impl Interface {
    pub fn new(name: &str) -> io::Result<Self> {
        // Create a dummy socket for ioctls
        let sock = socket(AddressFamily::Inet, SockType::Datagram, SockFlag::empty(), None)
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
        Err(io::Error::new(io::ErrorKind::NotFound, "Interface not found"))
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
            if flags & 0x1 != 0 { ret.push("UP"); }
            if flags & 0x2 != 0 { ret.push("BROADCAST"); }
            if flags & 0x4 != 0 { ret.push("DEBUG"); }
            if flags & 0x8 != 0 { ret.push("LOOPBACK"); }
            if flags & 0x10 != 0 { ret.push("POINTOPOINT"); }
            if flags & 0x20 != 0 { ret.push("SMART"); }
            if flags & 0x40 != 0 { ret.push("RUNNING"); }
            if flags & 0x80 != 0 { ret.push("NOARP"); }
            if flags & 0x100 != 0 { ret.push("PROMISC"); }
            if flags & 0x200 != 0 { ret.push("ALLMULTI"); }
            if flags & 0x400 != 0 { ret.push("OACTIVE"); }
            if flags & 0x800 != 0 { ret.push("SIMPLEX"); }
            if flags & 0x1000 != 0 { ret.push("LINK0"); }
            if flags & 0x2000 != 0 { ret.push("LINK1"); }
            if flags & 0x4000 != 0 { ret.push("LINK2"); }
            if flags & 0x8000 != 0 { ret.push("MULTICAST"); }
        }

        #[cfg(not(target_os = "macos"))]
        {
            // Standard Linux-like flags
            if flags & 0x1 != 0 { ret.push("UP"); }
            if flags & 0x2 != 0 { ret.push("BROADCAST"); }
            if flags & 0x4 != 0 { ret.push("DEBUG"); }
            if flags & 0x8 != 0 { ret.push("LOOPBACK"); }
            if flags & 0x10 != 0 { ret.push("PTP"); }
            if flags & 0x20 != 0 { ret.push("NOTRAILERS"); }
            if flags & 0x40 != 0 { ret.push("RUNNING"); }
            if flags & 0x80 != 0 { ret.push("NOARP"); }
            if flags & 0x100 != 0 { ret.push("PROMISC"); }
            if flags & 0x200 != 0 { ret.push("ALLMULTI"); }
            if flags & 0x400 != 0 { ret.push("MASTER"); }
            if flags & 0x800 != 0 { ret.push("SLAVE"); }
            if flags & 0x1000 != 0 { ret.push("MULTICAST"); }
            if flags & 0x2000 != 0 { ret.push("PORTSEL"); }
            if flags & 0x4000 != 0 { ret.push("AUTOMEDIA"); }
            if flags & 0x8000 != 0 { ret.push("DYNAMIC"); }
        }

        SmolStr::from(ret.join(" "))
    }

    pub fn ethtool_drvinfo(&self) -> io::Result<EthtoolDrvInfo> {
        #[cfg(target_os = "linux")]
        {
            let mut info: EthtoolDrvInfo = Default::default();
            info.cmd = ETHTOOL_GDRVINFO;

            let mut req = IfReq::new(&self.name);
            req.ifr_ifru.ifru_data = &mut info as *mut _ as *mut c_void;

            unsafe { ioctl_ethtool(self.sock.as_raw_fd(), &mut req) }.map_err(|e| io::Error::from_raw_os_error(e as i32))?;
            Ok(info)
        }

        #[cfg(target_os = "macos")]
        {
            if let Some((driver, version, bus)) = macos::get_driver_info(&self.name) {
                let mut info: EthtoolDrvInfo = Default::default();
                // Copy strings to info.driver, info.version, info.bus_info
                let copy_str = |dest: &mut [c_char], src: &str| {
                    let bytes = src.as_bytes();
                    for (i, b) in bytes.iter().enumerate() {
                        if i >= dest.len() - 1 { break; }
                        dest[i] = *b as c_char;
                    }
                    // Null terminate if space permits or if truncated
                    let last_idx = std::cmp::min(bytes.len(), dest.len() - 1);
                    dest[last_idx] = 0;
                };

                copy_str(&mut info.driver, &driver);
                copy_str(&mut info.version, &version);
                copy_str(&mut info.bus_info, &bus);

                Ok(info)
            } else {
                Err(io::Error::new(io::ErrorKind::NotFound, "Driver info not found"))
            }
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(io::Error::new(io::ErrorKind::Unsupported, "Not supported on this OS"))
        }
    }

    pub fn media(&self) -> io::Result<SmolStr> {
        #[cfg(target_os = "linux")]
        {
            let mut cmd: EthtoolCmd = Default::default();
            cmd.cmd = ETHTOOL_GSET;

            let mut req = IfReq::new(&self.name);
            req.ifr_ifru.ifru_data = &mut cmd as *mut _ as *mut libc::c_void;

            if unsafe { ioctl_ethtool(self.sock.as_raw_fd(), &mut req) }.is_ok() {
                let speed = (cmd.speed_hi as u32) << 16 | (cmd.speed as u32);
                let duplex = if cmd.duplex == 0x01 { "full" } else if cmd.duplex == 0x00 { "half" } else { "unknown" };
                let port = match cmd.port {
                    0x00 => "TP",
                    0x01 => "AUI",
                    0x02 => "MII",
                    0x03 => "FIBRE",
                    0x04 => "BNC",
                    _ => "unknown",
                };
                if speed == 0 || speed == 0xFFFF || speed == 0xFFFFFFFF {
                    return Ok(format!("{} (unknown speed)", port).into());
                }
                return Ok(format!("{} {}Mb/s {}", port, speed, duplex).into());
            }
            Ok(SmolStr::new_static("unknown"))
        }

        #[cfg(target_os = "macos")]
        {
            let mut req: IfMediaReq = unsafe { std::mem::zeroed() };
            let bytes = self.name.as_bytes();
            let len = std::cmp::min(bytes.len(), 15);
            for (i, &byte) in bytes.iter().enumerate().take(len) {
                req.ifm_name[i] = byte as libc::c_char;
            }

            // On macOS, SIOCGIFMEDIA often requires a larger buffer or specific socket.
            // We use a dummy list to ensure the structure is fully populated if needed.
            let mut res = unsafe { ioctl_get_media(self.sock.as_raw_fd(), &mut req) };

            if res.is_err() {
                // Try with different socket families as some drivers (like Wi-Fi) are picky
                for family in [libc::AF_INET6, libc::AF_LINK] {
                    let s = unsafe { libc::socket(family, libc::SOCK_DGRAM, 0) };
                    if s >= 0 {
                        res = unsafe { ioctl_get_media(s, &mut req) };
                        unsafe { libc::close(s) };
                        if res.is_ok() { break; }
                    }
                }
            }

            if res.is_ok() {
                let active = req.ifm_active;
                let type_ = (active & 0x000000f0) >> 4;
                let subtype = active & 0x0000000f;

                let type_str = match type_ {
                    2 => "Ethernet",
                    8 => "Wi-Fi",
                    _ => "Other",
                };

                let subtype_str = if type_ == 2 {
                    match subtype {
                        0 => "autoselect",
                        3 => "10baseT",
                        6 => "100baseTX",
                        12 => "1000baseT",
                        19 => "10GbaseT",
                        _ => "unknown",
                    }
                } else if type_ == 8 {
                    match subtype {
                        0 => "autoselect",
                        3 => "802.11b",
                        4 => "802.11g",
                        5 => "802.11a",
                        6 => "802.11n",
                        8 => "802.11ac",
                        11 => "802.11ax",
                        _ => "unknown",
                    }
                } else {
                    "unknown"
                };

                let mut options = Vec::new();
                if active & 0x00010000 != 0 { options.push("full-duplex"); }
                if active & 0x00020000 != 0 { options.push("half-duplex"); }

                if options.is_empty() {
                    use smol_str::format_smolstr;

                    return Ok(format_smolstr!("{} {}", type_str, subtype_str));
                } else {
                    use smol_str::format_smolstr;

                    return Ok(format_smolstr!("{} {} <{}>", type_str, subtype_str, options.join(",")));
                }
            }

            // Fallback for macOS: use networksetup if ioctl fails or returns generic info
            let output = std::process::Command::new("networksetup")
                .arg("-getmedia")
                .arg(&*self.name)
                .output();

            if let Ok(out) = output {
                let s = String::from_utf8_lossy(&out.stdout);
                for line in s.lines() {
                    if line.starts_with("Current:") {
                        let val = line.trim_start_matches("Current:").trim();
                        if val != "autoselect" && !val.is_empty() {
                            return Ok(val.into());
                        }
                    }
                }
            }

            Ok(SmolStr::new_static("unknown"))
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Ok("unknown".to_string())
        }
    }



    pub fn ethtool_link(&self) -> io::Result<bool> {
        #[cfg(target_os = "linux")]
        {
            let mut val = EthtoolValue {
                cmd: ETHTOOL_GLINK,
                ..Default::default()
            };

            let mut req = IfReq::new(&self.name);
            req.ifr_ifru.ifru_data = &mut val as *mut _ as *mut c_void;

            unsafe { ioctl_ethtool(self.sock.as_raw_fd(), &req) }.map_err(|e| io::Error::from_raw_os_error(e as i32))?;
            Ok(val.data != 0)
        }

        #[cfg(target_os = "macos")]
        {
            let mut req: IfMediaReq = unsafe { std::mem::zeroed() };
            let bytes = self.name.as_bytes();
            let len = std::cmp::min(bytes.len(), 15);
            for (i, &byte) in bytes.iter().enumerate().take(len) {
                req.ifm_name[i] = byte as c_char;
            }

            match unsafe { ioctl_get_media(self.sock.as_raw_fd(), &mut req) } {
                Ok(_) => {
                    // IFM_AVALID = 0x00000001, IFM_ACTIVE = 0x00000002
                    Ok((req.ifm_status & 0x00000001 != 0) && (req.ifm_status & 0x00000002 != 0))
                }
                Err(_) => {
                    // Fallback to IFF_RUNNING if SIOCGIFMEDIA is not supported (e.g. some virtual interfaces)
                    Ok(self.flags().map(|f| f & 0x40 != 0).unwrap_or(false))
                }
            }
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(io::Error::new(io::ErrorKind::Unsupported, "Not supported on this OS"))
        }
    }

    #[cfg(target_os = "linux")]
    pub fn mac(&self) -> io::Result<SmolStr> {
        use smol_str::format_smolstr;

        let mut req = IfReq::new(&self.name);
        unsafe { ioctl_get_hwaddr(self.sock.as_raw_fd(), &mut req) }.map_err(|e| io::Error::from_raw_os_error(e as i32))?;
        let addr = unsafe { req.ifr_ifru.ifru_hwaddr.sa_data };
        // sa_data is [i8; 14]. MAC is first 6.
        Ok(format_smolstr!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            addr[0] as u8, addr[1] as u8, addr[2] as u8,
            addr[3] as u8, addr[4] as u8, addr[5] as u8))
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

                             let s = addr.iter().map(|b| format_smolstr!("{:02x}", b)).collect::<Vec<_>>().join(":");
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
        unsafe { ioctl_get_mtu(self.sock.as_raw_fd(), &mut req) }.map_err(|e| io::Error::from_raw_os_error(e as i32))?;
        unsafe { Ok(req.ifr_ifru.ifru_mtu) }
    }

    pub fn metric(&self) -> io::Result<i32> {
        let mut req = IfReq::new(&self.name);
        match unsafe { ioctl_get_metric(self.sock.as_raw_fd(), &mut req) } {
            Ok(_) => unsafe { Ok(if req.ifr_ifru.ifru_ivalue == 0 { 1 } else { req.ifr_ifru.ifru_ivalue }) },
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

                             let mask_opt = ifa.netmask.as_ref().and_then(|a| a.as_sockaddr_in().map(|s| s.ip()));

                             if let Some(mask_u32) = mask_opt {
                                 let mask_ip = std::net::Ipv4Addr::from(mask_u32);
                                 // Mask is u32.
                                 let prefix = u32::from(mask_ip).count_ones() as i32;

                                 ret.push((SmolStr::from(ip.to_string()), SmolStr::from(mask_ip.to_string()), prefix));
                             } else {
                                 ret.push((SmolStr::from(ip.to_string()), SmolStr::new_static(""), 0));
                             }
                         }
                    }
                }
            }
        }
        ret
    }
}
