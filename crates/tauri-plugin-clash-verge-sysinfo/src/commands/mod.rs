use parking_lot::RwLock;
use serde::Serialize;
use sysinfo::Disks;
use sysinfo::System;
use tauri::{AppHandle, Runtime, State, command};
use tauri_plugin_clipboard_manager::{ClipboardExt as _, Error};

use crate::Platform;

#[cfg(windows)]
mod windows_display;
#[cfg(windows)]
mod windows_hw;

/// CPU / 内存信息（屏幕分辨率在 WebView 中用 `window.screen` 读取）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareInfo {
    /// CPU 品牌与型号（如含 Intel / AMD 等）
    pub cpu_brand: String,
    pub cpu_logical_cores: usize,
    pub cpu_physical_cores: Option<usize>,
    /// 物理内存总量（字节）
    pub total_memory_bytes: u64,
    /// 当前可用内存（字节，依系统统计口径略有差异）
    pub available_memory_bytes: u64,
}

/// 单块磁盘（卷 / 物理盘标识，依平台而定）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareDiskInfo {
    pub name: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

/// 显卡名称、厂商（`AdapterCompatibility`）与显存（`AdapterRAM`，部分驱动可能不准或为 0）
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WindowsGpuAdapter {
    /// WMI `AdapterCompatibility`，芯片厂商（NVIDIA / AMD 等），保留
    pub manufacturer: Option<String>,
    /// 由 `PNPDeviceID` 中 SUBSYS 子系统厂商 ID 映射的板卡品牌（华硕/技嘉/七彩虹等），无法识别时为 `None`
    pub board_brand: Option<String>,
    /// 子系统厂商 ID（十六进制可对照 PCI 数据库），未解析到 SUBSYS 时为 `None`
    pub subsystem_vendor_id: Option<u16>,
    pub name: String,
    pub adapter_ram_bytes: Option<u64>,
}

/// 物理磁盘（`MSFT_PhysicalDisk`）与介质类型
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsPhysicalDisk {
    pub friendly_name: String,
    pub size_bytes: u64,
    /// 如 `SSD` / `HDD` / `Unspecified` / `SCM` / `Other`
    pub media_type: String,
}

/// 内存条 DIMM（`Win32_PhysicalMemory`）
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WindowsMemoryModule {
    /// 制造商字符串（部分机器为 JEDEC ID / 十六进制，依 BIOS 上报为准）
    pub manufacturer: Option<String>,
    /// SMBIOS Memory Device Type 原始值
    pub smbios_memory_type: Option<u32>,
    /// 可读类型，如 `DDR4` / `DDR5` / `LPDDR4`（无法识别时为 `Unknown`）
    pub memory_standard: String,
    pub speed_mhz: Option<u32>,
    pub configured_clock_speed_mhz: Option<u32>,
    pub capacity_bytes: Option<u64>,
}

/// CPU 主频（`Win32_Processor`，MHz）
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WindowsCpuClocks {
    pub name: Option<String>,
    pub max_clock_speed_mhz: Option<u32>,
    pub current_clock_speed_mhz: Option<u32>,
}

/// 仅 Windows：`ROOT\\WMI` 显示器信息；其他平台不返回。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsDisplayMonitor {
    pub instance_name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    /// EDID 中的最大图像水平尺寸（cm），0 表示未知
    pub width_cm: Option<u8>,
    pub height_cm: Option<u8>,
    /// 由宽、高（cm）按矩形对角线推算的英寸（非官方标称「寸」时可能与包装标注略有差异）
    pub diagonal_inches: Option<f64>,
}

/// 仅 Windows：WMI + 磁盘枚举；其他平台返回空字段。
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WindowsHardwareExtra {
    pub disks: Vec<HardwareDiskInfo>,
    /// 显卡名称列表（与 `gpu_adapters` 名称一致，便于兼容旧逻辑）
    pub gpus: Vec<String>,
    pub gpu_adapters: Vec<WindowsGpuAdapter>,
    /// 物理盘 SSD/HDD 等（与卷 `disks` 不同维度）
    pub physical_disks: Vec<WindowsPhysicalDisk>,
    pub memory_modules: Vec<WindowsMemoryModule>,
    pub cpu_clocks: Option<WindowsCpuClocks>,
    pub network_adapters: Vec<String>,
    pub motherboard_manufacturer: Option<String>,
    pub motherboard_product: Option<String>,
}

/// 获取 CPU 型号与内存容量
#[command]
pub fn get_hardware_info() -> Result<HardwareInfo, String> {
    let mut sys = System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let cpu_brand = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_default();
    let cpu_logical_cores = sys.cpus().len();
    let cpu_physical_cores = System::physical_core_count();
    let total_memory_bytes = sys.total_memory();
    let available_memory_bytes = sys.available_memory();

    Ok(HardwareInfo {
        cpu_brand,
        cpu_logical_cores,
        cpu_physical_cores,
        total_memory_bytes,
        available_memory_bytes,
    })
}

/// 跨平台：磁盘列表与容量（字节）
///
/// - Windows：返回卷列表（与 `get_windows_hardware_extra().physical_disks` 的物理盘维度不同）
/// - macOS/Linux：返回系统枚举到的磁盘/卷
#[command]
pub fn get_disks() -> Result<Vec<HardwareDiskInfo>, String> {
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
    disks.dedup_by(|a, b| a.name == b.name && a.total_bytes == b.total_bytes);
    Ok(disks)
}

/// Windows：显示器厂商/型号（WMI）与物理尺寸推算对角线英寸。非 Windows 返回空数组。
#[command]
#[allow(clippy::missing_const_for_fn)]
pub fn get_windows_displays() -> Result<Vec<WindowsDisplayMonitor>, String> {
    #[cfg(windows)]
    {
        windows_display::query_displays()
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

/// Windows：硬盘容量列表、显卡名称、物理网卡、主板厂商/型号。非 Windows 返回空结构。
#[command]
pub fn get_windows_hardware_extra() -> Result<WindowsHardwareExtra, String> {
    #[cfg(windows)]
    {
        windows_hw::query()
    }
    #[cfg(not(windows))]
    {
        Ok(WindowsHardwareExtra::default())
    }
}

// TODO 迁移，让新的结构体允许通过 tauri command 正确使用 structure.field 方式获取信息
#[command]
pub fn get_system_info(state: State<'_, RwLock<Platform>>) -> Result<String, Error> {
    Ok(state.inner().read().to_string())
}

/// 获取应用的运行时间（毫秒）
#[command]
pub fn get_app_uptime(state: State<'_, RwLock<Platform>>) -> Result<u128, Error> {
    Ok(state.inner().read().appinfo.app_startup_time.elapsed().as_millis())
}

/// 检查应用是否以管理员身份运行
#[command]
pub fn app_is_admin(state: State<'_, RwLock<Platform>>) -> Result<bool, Error> {
    Ok(state.inner().read().appinfo.app_is_admin)
}

#[command]
pub fn export_diagnostic_info<R: Runtime>(
    app_handle: AppHandle<R>,
    state: State<'_, RwLock<Platform>>,
) -> Result<(), Error> {
    let info = state.inner().read().to_string();
    let clipboard = app_handle.clipboard();
    clipboard.write_text(info)
}
