//! Windows-only: WMI + sysinfo disks for extended hardware info.

use std::collections::HashMap;

use serde::Deserialize;
use sysinfo::Disks;
use wmi::{Variant, WMIConnection};

use super::{
    HardwareDiskInfo, WindowsCpuClocks, WindowsGpuAdapter, WindowsHardwareExtra, WindowsMemoryModule,
    WindowsPhysicalDisk,
};

#[derive(Debug, Deserialize)]
struct WmiVideoController {
    #[serde(rename = "Name")]
    name: Option<String>,
    /// 芯片/驱动兼容厂商名（与 `Name` 可能部分重复）
    #[serde(rename = "AdapterCompatibility")]
    adapter_compatibility: Option<String>,
    /// 如 `PCI\VEN_10DE&DEV_xxxx&SUBSYS_xxxxxxxx&REV_xx`，用于解析板卡厂
    #[serde(rename = "PNPDeviceID")]
    pnp_device_id: Option<String>,
    /// 字节，部分显卡/驱动为 0 或不可靠
    #[serde(rename = "AdapterRAM")]
    adapter_ram: Option<u32>,
}

/// `PNPDeviceID` 里 `SUBSYS_xxxxxxxx` 的低 16 位常为 Subsystem Vendor ID。
fn subsystem_vendor_id_from_pnp(pnp: &str) -> Option<u16> {
    let upper = pnp.to_ascii_uppercase();
    let needle = "SUBSYS_";
    let idx = upper.find(needle)?;
    let start = idx + needle.len();
    let hex: String = upper[start..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 8 {
        return None;
    }
    let v = u32::from_str_radix(&hex, 16).ok()?;
    Some((v & 0xFFFF) as u16)
}

/// 常见显卡 AIB 子系统厂商（PCI ID），未收录则返回 `None`。
const fn pci_board_brand_name(vendor_id: u16) -> Option<&'static str> {
    match vendor_id {
        0x1043 => Some("ASUS"),
        0x1458 => Some("Gigabyte"),
        0x1462 => Some("MSI"),
        0x1849 => Some("ASRock"),
        0x19DA => Some("ZOTAC"),
        0x1B0A => Some("Palit"),
        0x1B4C => Some("Galax"),
        0x1DA2 => Some("Sapphire"),
        0x1DA5 => Some("PowerColor"),
        0x1682 => Some("XFX"),
        0x148C => Some("TUL"),
        0x1734 => Some("Clevo"),
        0x196D => Some("Club3D"),
        0x3842 => Some("EVGA"),
        0x7377 => Some("Colorful"),
        0x1028 => Some("Dell"),
        0x17AA => Some("Lenovo"),
        0x1025 => Some("Acer"),
        0x1022 => Some("AMD"),
        0x10DE => Some("NVIDIA"),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
struct WmiBaseBoard {
    #[serde(rename = "Manufacturer")]
    manufacturer: Option<String>,
    #[serde(rename = "Product")]
    product: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WmiNetworkAdapter {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "NetConnectionID")]
    net_connection_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WmiProcessor {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "MaxClockSpeed")]
    max_clock_speed: Option<u32>,
    #[serde(rename = "CurrentClockSpeed")]
    current_clock_speed: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct WmiPhysicalMemory {
    #[serde(rename = "Manufacturer")]
    manufacturer: Option<String>,
    #[serde(rename = "SMBIOSMemoryType")]
    smbios_memory_type: Option<u32>,
    #[serde(rename = "Speed")]
    speed: Option<u32>,
    #[serde(rename = "ConfiguredClockSpeed")]
    configured_clock_speed: Option<u32>,
    #[serde(rename = "Capacity")]
    capacity: Option<u64>,
}

/// DMTF SMBIOS Memory Device — Type（常见值，未列出则 Other）
const fn smbios_memory_standard(code: u32) -> &'static str {
    match code {
        0 => "Unknown",
        1 => "Other",
        2 => "DRAM",
        3 => "DRAM",
        4 => "EDRAM",
        5 => "VRAM",
        6 => "SRAM",
        7 => "RAM",
        8 => "ROM",
        9 => "FLASH",
        15 => "SDRAM",
        18 => "DDR",
        19 => "DDR2",
        20 => "DDR2 FB-DIMM",
        21 | 24 => "DDR3",
        22 => "FBD2",
        26 => "DDR4",
        27 => "LPDDR",
        28 => "LPDDR2",
        29 => "LPDDR3",
        30 => "LPDDR4",
        31 => "Logical non-volatile",
        32 => "HBM",
        33 => "HBM2",
        34 => "DDR5",
        35 => "LPDDR5",
        36 => "DDR5",
        _ => "Other",
    }
}

const fn media_type_label(code: u16) -> &'static str {
    match code {
        0 => "Unspecified",
        3 => "HDD",
        4 => "SSD",
        5 => "SCM",
        _ => "Other",
    }
}

fn variant_to_u16(v: &Variant) -> Option<u16> {
    match v {
        Variant::UI2(n) => Some(*n),
        Variant::UI4(n) => u16::try_from(*n).ok(),
        _ => None,
    }
}

const fn variant_to_u64(v: &Variant) -> Option<u64> {
    match v {
        Variant::UI8(n) => Some(*n),
        Variant::UI4(n) => Some(*n as u64),
        Variant::UI2(n) => Some(*n as u64),
        _ => None,
    }
}

fn variant_to_string(v: &Variant) -> Option<String> {
    match v {
        Variant::String(s) => {
            let t = s.trim();
            if t.is_empty() { None } else { Some(t.to_string()) }
        }
        _ => None,
    }
}

fn query_physical_disks() -> Vec<WindowsPhysicalDisk> {
    let Ok(wmi_stor) = WMIConnection::with_namespace_path("ROOT\\Microsoft\\Windows\\Storage") else {
        return Vec::new();
    };
    let Ok(rows) =
        wmi_stor.raw_query::<HashMap<String, Variant>>("SELECT FriendlyName, Size, MediaType FROM MSFT_PhysicalDisk")
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in rows {
        let Some(name) = row.get("FriendlyName").and_then(variant_to_string) else {
            continue;
        };
        let size = row.get("Size").and_then(variant_to_u64).unwrap_or(0);
        let mt = row
            .get("MediaType")
            .and_then(variant_to_u16)
            .map(media_type_label)
            .unwrap_or("Unspecified");
        out.push(WindowsPhysicalDisk {
            friendly_name: name,
            size_bytes: size,
            media_type: mt.to_string(),
        });
    }
    out.sort_by(|a, b| a.friendly_name.cmp(&b.friendly_name));
    out
}

pub(super) fn query() -> Result<WindowsHardwareExtra, String> {
    let wmi = WMIConnection::new().map_err(|e| e.to_string())?;

    let vc_rows: Vec<WmiVideoController> = wmi
        .raw_query(
            "SELECT Name, AdapterCompatibility, AdapterRAM, PNPDeviceID FROM Win32_VideoController \
             WHERE Name IS NOT NULL",
        )
        .map_err(|e| e.to_string())?;

    let mut gpu_adapters: Vec<WindowsGpuAdapter> = vc_rows
        .into_iter()
        .filter_map(|r| {
            let name = r.name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())?;
            let manufacturer = r.adapter_compatibility.and_then(|s| {
                let t = s.trim();
                if t.is_empty() { None } else { Some(t.to_string()) }
            });
            let subsystem_vendor_id = r.pnp_device_id.as_deref().and_then(subsystem_vendor_id_from_pnp);
            let board_brand = subsystem_vendor_id
                .and_then(pci_board_brand_name)
                .map(|s| s.to_string());
            let adapter_ram_bytes = r
                .adapter_ram
                .and_then(|b| if b == 0 || b == u32::MAX { None } else { Some(b as u64) });
            Some(WindowsGpuAdapter {
                manufacturer,
                board_brand,
                subsystem_vendor_id,
                name,
                adapter_ram_bytes,
            })
        })
        .collect();
    gpu_adapters.sort_by(|a, b| a.name.cmp(&b.name));
    gpu_adapters.dedup_by(|a, b| a.name == b.name);

    let gpus: Vec<String> = gpu_adapters.iter().map(|g| g.name.clone()).collect();

    let board = wmi
        .raw_query::<WmiBaseBoard>("SELECT Manufacturer, Product FROM Win32_BaseBoard")
        .map_err(|e| e.to_string())?
        .into_iter()
        .next();

    let motherboard_manufacturer = board
        .as_ref()
        .and_then(|b| b.manufacturer.as_ref())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let motherboard_product = board
        .as_ref()
        .and_then(|b| b.product.as_ref())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let adapters_raw: Vec<WmiNetworkAdapter> = wmi
        .raw_query(
            "SELECT Name, NetConnectionID FROM Win32_NetworkAdapter \
             WHERE PhysicalAdapter=TRUE AND NetConnectionID IS NOT NULL",
        )
        .map_err(|e| e.to_string())?;

    let mut network_adapters: Vec<String> = adapters_raw
        .into_iter()
        .filter_map(|a| match (a.name, a.net_connection_id) {
            (Some(n), Some(id)) => {
                let n = n.trim();
                let id = id.trim();
                if n.is_empty() {
                    None
                } else if id.is_empty() {
                    Some(n.to_string())
                } else {
                    Some(format!("{n} [{id}]"))
                }
            }
            (Some(n), None) => {
                let n = n.trim();
                if n.is_empty() { None } else { Some(n.to_string()) }
            }
            _ => None,
        })
        .collect();
    network_adapters.sort();
    network_adapters.dedup();

    let sys_disks = Disks::new_with_refreshed_list();
    let mut disks: Vec<HardwareDiskInfo> = sys_disks
        .list()
        .iter()
        .map(|d| HardwareDiskInfo {
            name: d.name().to_string_lossy().into_owned(),
            total_bytes: d.total_space(),
            available_bytes: d.available_space(),
        })
        .filter(|d| d.total_bytes > 0)
        .collect();
    disks.sort_by(|a, b| a.name.cmp(&b.name));

    let cpu_clocks = wmi
        .raw_query::<WmiProcessor>("SELECT Name, MaxClockSpeed, CurrentClockSpeed FROM Win32_Processor")
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .map(|p| WindowsCpuClocks {
            name: p.name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            max_clock_speed_mhz: p.max_clock_speed,
            current_clock_speed_mhz: p.current_clock_speed,
        });

    let memory_modules: Vec<WindowsMemoryModule> = wmi
        .raw_query::<WmiPhysicalMemory>(
            "SELECT Manufacturer, SMBIOSMemoryType, Speed, ConfiguredClockSpeed, Capacity \
             FROM Win32_PhysicalMemory",
        )
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            let manufacturer = m.manufacturer.and_then(|s| {
                let t = s.trim();
                if t.is_empty() { None } else { Some(t.to_string()) }
            });
            let memory_standard = m
                .smbios_memory_type
                .map(smbios_memory_standard)
                .unwrap_or("Unknown")
                .to_string();
            WindowsMemoryModule {
                manufacturer,
                smbios_memory_type: m.smbios_memory_type,
                memory_standard,
                speed_mhz: m.speed,
                configured_clock_speed_mhz: m.configured_clock_speed,
                capacity_bytes: m.capacity,
            }
        })
        .collect();

    let physical_disks = query_physical_disks();

    Ok(WindowsHardwareExtra {
        disks,
        gpus,
        gpu_adapters,
        physical_disks,
        memory_modules,
        cpu_clocks,
        network_adapters,
        motherboard_manufacturer,
        motherboard_product,
    })
}
