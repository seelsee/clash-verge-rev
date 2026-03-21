//! Windows-only: WMI + sysinfo disks for extended hardware info.

use serde::Deserialize;
use sysinfo::Disks;
use wmi::WMIConnection;

use super::{HardwareDiskInfo, WindowsHardwareExtra};

#[derive(Debug, Deserialize)]
struct WmiVideoController {
    #[serde(rename = "Name")]
    name: Option<String>,
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

pub(super) fn query() -> Result<WindowsHardwareExtra, String> {
    let wmi = WMIConnection::new().map_err(|e| e.to_string())?;

    let mut gpus: Vec<String> = wmi
        .raw_query::<WmiVideoController>("SELECT Name FROM Win32_VideoController WHERE Name IS NOT NULL")
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|r| r.name)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    gpus.sort();
    gpus.dedup();

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
        })
        .filter(|d| d.total_bytes > 0)
        .collect();
    disks.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(WindowsHardwareExtra {
        disks,
        gpus,
        network_adapters,
        motherboard_manufacturer,
        motherboard_product,
    })
}
