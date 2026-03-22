//! Windows-only: 显示器友好名称 / 厂商 + EDID 物理尺寸（cm）推算对角线英寸（`ROOT\\WMI`）。

use std::collections::HashMap;

use wmi::{Variant, WMIConnection};

use super::WindowsDisplayMonitor;

fn variant_as_str(v: &Variant) -> Option<String> {
    match v {
        Variant::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn variant_as_u8(v: &Variant) -> Option<u8> {
    match v {
        Variant::UI1(n) => Some(*n),
        Variant::UI2(n) => u8::try_from(*n).ok(),
        _ => None,
    }
}

/// `WmiMonitorID` 中 ManufacturerName / UserFriendlyName 常为 UInt8[]（UTF-16 LE）或字符串。
fn variant_to_monitor_string(v: &Variant) -> Option<String> {
    match v {
        Variant::String(s) => {
            let t = s.trim();
            if t.is_empty() { None } else { Some(t.to_string()) }
        }
        Variant::Array(items) => {
            let bytes: Vec<u8> = items
                .iter()
                .filter_map(|x| match x {
                    Variant::UI1(b) => Some(*b),
                    _ => None,
                })
                .collect();
            if bytes.is_empty() {
                return None;
            }
            decode_wmi_utf16_byte_blob(&bytes)
        }
        _ => None,
    }
}

fn decode_wmi_utf16_byte_blob(bytes: &[u8]) -> Option<String> {
    if bytes.len() >= 2 && bytes.len().is_multiple_of(2) {
        let u16s: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let end = u16s.iter().position(|&c| c == 0).unwrap_or(u16s.len());
        let s = String::from_utf16(&u16s[..end]).ok()?;
        let t = s.trim();
        if t.is_empty() { None } else { Some(t.to_string()) }
    } else {
        let s = String::from_utf8(bytes.to_vec()).ok()?;
        let t = s.trim().trim_matches('\0').trim();
        if t.is_empty() { None } else { Some(t.to_string()) }
    }
}

fn diagonal_inches_from_cm(w: u8, h: u8) -> Option<f64> {
    if w == 0 || h == 0 {
        return None;
    }
    let d_cm = ((w as f64).powi(2) + (h as f64).powi(2)).sqrt();
    Some((d_cm / 2.54 * 10.0).round() / 10.0)
}

pub(super) fn query_displays() -> Result<Vec<WindowsDisplayMonitor>, String> {
    let wmi = WMIConnection::with_namespace_path("ROOT\\WMI").map_err(|e| e.to_string())?;

    let basic_rows: Vec<HashMap<String, Variant>> = wmi
        .raw_query(
            "SELECT InstanceName, MaxHorizontalImageSize, MaxVerticalImageSize \
             FROM WmiMonitorBasicDisplayParams",
        )
        .map_err(|e| e.to_string())?;

    let mut sizes: HashMap<String, (u8, u8)> = HashMap::new();
    for row in basic_rows {
        let inst = row.get("InstanceName").and_then(variant_as_str);
        let h = row.get("MaxHorizontalImageSize").and_then(variant_as_u8);
        let v = row.get("MaxVerticalImageSize").and_then(variant_as_u8);
        if let (Some(name), Some(h), Some(v)) = (inst, h, v) {
            sizes.insert(name, (h, v));
        }
    }

    let id_rows: Vec<HashMap<String, Variant>> = wmi
        .raw_query("SELECT InstanceName, ManufacturerName, UserFriendlyName FROM WmiMonitorID")
        .map_err(|e| e.to_string())?;

    let mut names: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
    for row in id_rows {
        let inst = row.get("InstanceName").and_then(variant_as_str);
        let man = row.get("ManufacturerName").and_then(variant_to_monitor_string);
        let model = row.get("UserFriendlyName").and_then(variant_to_monitor_string);
        if let Some(name) = inst {
            names.insert(name, (man, model));
        }
    }

    let mut keys: Vec<String> = sizes.keys().chain(names.keys()).cloned().collect();
    keys.sort();
    keys.dedup();

    let mut out = Vec::with_capacity(keys.len());
    for instance_name in keys {
        let (w, h) = sizes.get(&instance_name).copied().unwrap_or((0, 0));
        let (manufacturer, model) = names.get(&instance_name).cloned().unwrap_or((None, None));
        let width_cm = (w > 0).then_some(w);
        let height_cm = (h > 0).then_some(h);
        let diagonal_inches = diagonal_inches_from_cm(w, h);
        out.push(WindowsDisplayMonitor {
            instance_name,
            manufacturer,
            model,
            width_cm,
            height_cm,
            diagonal_inches,
        });
    }

    Ok(out)
}
