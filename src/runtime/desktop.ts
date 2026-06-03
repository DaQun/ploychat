import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export interface ViewBounds {
  x: number
  y: number
  width: number
  height: number
  // React 视口逻辑高度(window.innerHeight)，供 Rust 推算标题栏偏移
  viewportHeight?: number
}

export interface PlatformViewState {
  platformId: string
  title: string
  canGoBack: boolean
  canGoForward: boolean
  loading: boolean
  url: string
}

export interface OpenTabRequest {
  platformId: string
  openerViewId: string
  url: string
}

export interface ShortcutEvent {
  action: 'switch-platform' | 'switch-tab' | 'switch-conversation' | string
  index?: number
  offset?: number
}

export interface DownloadFinishedEvent {
  platformId: string
  url: string
  filename: string | null
  path: string | null
  success: boolean
}

export type NavigationAction = 'back' | 'forward' | 'reload'

export async function createPlatformView(
  platformId: string,
  platformName: string,
  url: string,
  bounds: ViewBounds,
  userAgent?: string,
  storageId?: string
) {
  return invoke<PlatformViewState>('create_platform_view', {
    platformId,
    platformName,
    url,
    bounds,
    userAgent,
    storageId,
  })
}

export async function showPlatformView(platformId: string) {
  return invoke<void>('show_platform_view', { platformId })
}

export async function closePlatformView(platformId: string) {
  return invoke<void>('close_platform_view', { platformId })
}

export async function hideAllPlatformViews() {
  return invoke<void>('hide_all_platform_views')
}

export async function setPlatformViewBounds(platformId: string, bounds: ViewBounds) {
  return invoke<void>('set_platform_view_bounds', { platformId, bounds })
}

export async function navigate(platformId: string, action: NavigationAction) {
  return invoke<void>('navigate', { platformId, action })
}

export async function openExternal(url: string) {
  return invoke<void>('open_external', { url })
}

export async function quitApp() {
  return invoke<void>('quit_app')
}

export async function clearPlatformData(platformId: string) {
  return invoke<void>('clear_platform_data', { platformId })
}

export async function getPlatformState(platformId: string) {
  return invoke<PlatformViewState>('get_platform_state', { platformId })
}

export async function switchConversation(platformId: string, offset: number) {
  return invoke<void>('switch_conversation', { platformId, offset })
}

export function onPlatformStateChanged(
  callback: (state: PlatformViewState) => void
) {
  return listen<PlatformViewState>('platform-state-changed', event => {
    callback(event.payload)
  })
}

export function onOpenTabRequested(
  callback: (request: OpenTabRequest) => void
) {
  return listen<OpenTabRequest>('platform-open-tab-requested', event => {
    callback(event.payload)
  })
}

export function onShortcut(callback: (event: ShortcutEvent) => void) {
  return listen<ShortcutEvent>('polychat-shortcut', event => {
    callback(event.payload)
  })
}

export function onDownloadFinished(
  callback: (event: DownloadFinishedEvent) => void
) {
  return listen<DownloadFinishedEvent>('platform-download-finished', event => {
    callback(event.payload)
  })
}
