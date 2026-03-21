use parking_lot::RwLock;
use serde::Serialize;
use sysinfo::System;
use tauri::{AppHandle, Runtime, State, command};
use tauri_plugin_clipboard_manager::{ClipboardExt as _, Error};

use crate::Platform;

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
}

/// 单块磁盘（卷 / 物理盘标识，依平台而定）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareDiskInfo {
    pub name: String,
    pub total_bytes: u64,
}

/// 仅 Windows：WMI + 磁盘枚举；其他平台返回空字段。
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WindowsHardwareExtra {
    pub disks: Vec<HardwareDiskInfo>,
    pub gpus: Vec<String>,
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

    Ok(HardwareInfo {
        cpu_brand,
        cpu_logical_cores,
        cpu_physical_cores,
        total_memory_bytes,
    })
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
    Ok(state
        .inner()
        .read()
        .appinfo
        .app_startup_time
        .elapsed()
        .as_millis())
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
