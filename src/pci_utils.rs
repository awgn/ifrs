#[cfg(not(target_os = "macos"))]
use anyhow::Result;
#[cfg(all(feature = "pci-info", not(target_os = "macos")))]
use smol_str::SmolStr;
#[cfg(not(target_os = "macos"))]
use std::collections::HashMap;

#[cfg(not(target_os = "macos"))]
struct PciDb {
    vendors: HashMap<u16, String>,
    devices: HashMap<(u16, u16), String>,
}

#[cfg(not(target_os = "macos"))]
impl PciDb {
    fn new() -> Self {
        let mut db = PciDb {
            vendors: HashMap::new(),
            devices: HashMap::new(),
        };
        db.load();
        db
    }

    fn load(&mut self) {
        let paths = [
            "/usr/share/hwdata/pci.ids",
            "/usr/share/misc/pci.ids",
            "/usr/share/pci.ids",
        ];

        for path in paths {
            if let Ok(content) = std::fs::read_to_string(path) {
                self.parse(&content);
                return;
            }
        }
    }

    fn parse(&mut self, content: &str) {
        let mut current_vendor: Option<u16> = None;

        for line in content.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }

            if !line.starts_with('\t') {
                // Vendor
                let parts: Vec<&str> = line.splitn(2, ' ').collect();
                if parts.len() == 2 {
                    if let Ok(id) = u16::from_str_radix(parts[0], 16) {
                        current_vendor = Some(id);
                        self.vendors.insert(id, parts[1].trim().to_string());
                    }
                }
            } else if line.starts_with('\t') && !line.starts_with("\t\t") {
                // Device
                if let Some(vendor_id) = current_vendor {
                    let line = &line[1..];
                    let parts: Vec<&str> = line.splitn(2, ' ').collect();
                    if parts.len() == 2 {
                        if let Ok(dev_id) = u16::from_str_radix(parts[0], 16) {
                            self.devices.insert((vendor_id, dev_id), parts[1].trim().to_string());
                        }
                    }
                }
            }
        }
    }

    fn get_vendor(&self, id: u16) -> Option<String> {
        self.vendors.get(&id).cloned()
    }

    fn get_device(&self, vendor: u16, device: u16) -> Option<String> {
        self.devices.get(&(vendor, device)).cloned()
    }
}

#[derive(Debug, Clone, Default)]
pub struct PciDeviceInfo {
    pub vendor_id: u16,
    pub device_id: u16,
    pub vendor_name: Option<String>,
    pub device_name: Option<String>,
    pub subsystem_vendor: Option<u16>,
    pub subsystem_device: Option<u16>,
    pub class: Option<u8>,
    pub subclass: Option<u8>,
    pub revision: Option<u8>,
    pub bus: Option<u8>,
    pub device: Option<u8>,
    pub function: Option<u8>,
    pub driver: Option<String>,
    #[allow(dead_code)]
    pub numa_node: Option<i32>,
    #[allow(dead_code)]
    pub irq: Option<u32>,
}

impl PciDeviceInfo {
    #[allow(dead_code)]
    pub fn format_class(&self) -> String {
        if let (Some(class), Some(subclass)) = (self.class, self.subclass) {
            match (class, subclass) {
                (0x02, 0x00) => "Ethernet controller".to_string(),
                (0x02, 0x80) => "Network controller".to_string(),
                (0x0d, 0x11) => "802.1a controller".to_string(),
                (0x0d, 0x20) => "802.11b controller".to_string(),
                (0x0d, 0x80) => "Wireless controller".to_string(),
                _ => format!("Class {:02x}:{:02x}", class, subclass),
            }
        } else {
            "Unknown".to_string()
        }
    }

    pub fn pci_address(&self) -> Option<String> {
        if let (Some(bus), Some(dev), Some(func)) = (self.bus, self.device, self.function) {
            Some(format!("{:02x}:{:02x}.{}", bus, dev, func))
        } else {
            None
        }
    }
}

#[cfg(all(feature = "pci-info", not(target_os = "macos")))]
pub fn get_pci_devices() -> Result<HashMap<SmolStr, PciDeviceInfo>> {
    use pci_info::PciInfo;

    let mut devices = HashMap::new();
    let db = PciDb::new();

    match PciInfo::enumerate_pci() {
        Ok(pci_devices) => {
            for dev_result in pci_devices {
                let dev = match dev_result {
                    Ok(d) => d,
                    Err(_) => continue,
                };

                let class = match dev.device_class() {
                    Ok(c) => u8::from(c),
                    Err(_) => continue,
                };
                let subclass = match dev.device_subclass() {
                    Ok(sc) => u8::from(sc),
                    Err(_) => continue,
                };

                if class != 0x02 && class != 0x0d {
                    continue;
                }

                let location = dev.location();
                let (bus, device, function) = match location {
                    Ok(loc) => (Some(loc.bus()), Some(loc.device()), Some(loc.function())),
                    Err(_) => (None, None, None),
                };

                let mut info = PciDeviceInfo {
                    vendor_id: dev.vendor_id(),
                    device_id: dev.device_id(),
                    class: Some(class),
                    subclass: Some(subclass),
                    revision: dev.revision().ok(),
                    bus,
                    device,
                    function,
                    ..Default::default()
                };

                info.vendor_name = db.get_vendor(info.vendor_id);
                info.device_name = db.get_device(info.vendor_id, info.device_id);

                info.subsystem_vendor = dev.subsystem_vendor_id().ok().flatten();
                info.subsystem_device = dev.subsystem_device_id().ok().flatten();

                if let (Some(b), Some(d), Some(f)) = (bus, device, function) {
                    use smol_str::format_smolstr;

                    let key = format_smolstr!("{:02x}:{:02x}.{}", b, d, f);
                    devices.insert(key, info);
                }
            }
        }
        Err(e) => {
            eprintln!("Warning: Could not enumerate PCI devices: {}", e);
        }
    }

    Ok(devices)
}

#[cfg(all(not(feature = "pci-info"), not(target_os = "macos")))]
pub fn get_pci_devices() -> Result<HashMap<String, PciDeviceInfo>> {
    Ok(HashMap::new())
}

#[cfg(not(target_os = "macos"))]
pub fn find_pci_info_for_interface(
    interface_name: &str,
    bus_info: &str,
    pci_devices: &HashMap<SmolStr, PciDeviceInfo>,
) -> Option<PciDeviceInfo> {
    if bus_info.is_empty() {
        return None;
    }

    let clean_bus = bus_info.trim_start_matches("pci@");

    let pci_addr = if let Some(addr) = parse_pci_address(clean_bus) {
        addr
    } else if let Some(addr) = extract_pci_from_sysfs(interface_name) {
        addr
    } else {
        return None;
    };

    pci_devices.get(&pci_addr).cloned()
}

#[cfg(not(target_os = "macos"))]
fn parse_pci_address(bus_info: &str) -> Option<SmolStr> {
    let parts: Vec<&str> = bus_info.split(':').collect();

    if parts.len() >= 2 {
        let last_part = parts[parts.len() - 1];
        let second_last = parts[parts.len() - 2];

        if last_part.contains('.') {
            let dev_func: Vec<&str> = last_part.split('.').collect();
            if dev_func.len() == 2 {
                if let Ok(bus) = u8::from_str_radix(second_last, 16) {
                    if let Ok(dev) = u8::from_str_radix(dev_func[0], 16) {
                        if let Ok(func) = u8::from_str_radix(dev_func[1], 16) {
                            use smol_str::format_smolstr;

                            return Some(format_smolstr!("{:02x}:{:02x}.{}", bus, dev, func));
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn extract_pci_from_sysfs(interface_name: &str) -> Option<SmolStr> {
    use std::fs;
    use std::path::PathBuf;

    let sysfs_path = PathBuf::from(format!("/sys/class/net/{}/device/uevent", interface_name));

    if let Ok(content) = fs::read_to_string(&sysfs_path) {
        for line in content.lines() {
            if line.starts_with("PCI_SLOT_NAME=") {
                let addr = line.trim_start_matches("PCI_SLOT_NAME=");
                return parse_pci_address(addr);
            }
        }
    }

    let device_link = PathBuf::from(format!("/sys/class/net/{}/device", interface_name));
    if let Ok(target) = fs::read_link(&device_link) {
        if let Some(filename) = target.file_name() {
            if let Some(addr_str) = filename.to_str() {
                return parse_pci_address(addr_str);
            }
        }
    }

    None
}

#[cfg(all(not(target_os = "linux"), not(target_os = "macos")))]
fn extract_pci_from_sysfs(_interface_name: &str) -> Option<String> {
    None
}
