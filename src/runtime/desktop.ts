import { invoke } from '@tauri-apps/api/core'
import { LogicalPosition, LogicalSize } from '@tauri-apps/api/dpi'
import { listen } from '@tauri-apps/api/event'
import { Webview } from '@tauri-apps/api/webview'
import { getCurrentWindow } from '@tauri-apps/api/window'

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

const frontendViews = new Map<string, Webview>()
const frontendViewStates = new Map<string, PlatformViewState>()

function hasTauriRuntime() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

function rootPlatformIdForView(viewId: string) {
  const tabMarker = '__tab_'
  const markerIndex = viewId.indexOf(tabMarker)
  return markerIndex > 0 ? viewId.slice(0, markerIndex) : viewId
}

function shouldUseEphemeralFrontendStorage(platformId: string, storageId?: string) {
  const rootId = storageId ?? rootPlatformIdForView(platformId)
  return rootId.includes('__clone_')
}

function shouldUseFrontendWebview() {
  return hasTauriRuntime() && /Windows/i.test(navigator.userAgent)
}

async function installPlatformViewHooks(viewId: string, platformId = rootPlatformIdForView(viewId)) {
  return invoke<void>('install_platform_view_hooks', {
    viewId,
    platformId,
  })
}

function platformLabel(platformId: string) {
  return `platform-${platformId.replace(/[^a-zA-Z0-9_:/-]/g, '_')}`
}

function normalizeBounds(bounds: ViewBounds) {
  return {
    x: Math.max(0, Math.round(bounds.x)),
    y: Math.max(0, Math.round(bounds.y)),
    width: Math.max(16, Math.round(bounds.width)),
    height: Math.max(16, Math.round(bounds.height)),
  }
}

async function getFrontendView(platformId: string) {
  const existing = frontendViews.get(platformId)
  if (existing) return existing

  const view = await Webview.getByLabel(platformLabel(platformId))
  if (view) {
    frontendViews.set(platformId, view)
  }
  return view
}

function waitForWebviewCreated(view: Webview, timeoutMs = 12000) {
  return new Promise<void>((resolve, reject) => {
    let settled = false
    let cleanupCreated: (() => void) | undefined
    let cleanupError: (() => void) | undefined

    const settle = (callback: () => void) => {
      if (settled) return
      settled = true
      window.clearTimeout(timeoutId)
      cleanupCreated?.()
      cleanupError?.()
      callback()
    }

    const timeoutId = window.setTimeout(() => {
      settle(() => reject(new Error('Windows WebView 创建超时')))
    }, timeoutMs)

    view.once('tauri://created', () => {
      settle(resolve)
    }).then(unlisten => {
      cleanupCreated = unlisten
    }).catch(error => {
      settle(() => reject(error))
    })

    view.once<string>('tauri://error', event => {
      settle(() => reject(new Error(event.payload || 'Windows WebView 创建失败')))
    }).then(unlisten => {
      cleanupError = unlisten
    }).catch(error => {
      settle(() => reject(error))
    })
  })
}

async function createFrontendPlatformView(
  platformId: string,
  platformName: string,
  url: string,
  bounds: ViewBounds,
  userAgent?: string,
  storageId?: string
) {
  const label = platformLabel(platformId)
  const normalized = normalizeBounds(bounds)
  const existing = await getFrontendView(platformId)

  if (existing) {
    await existing.setPosition(new LogicalPosition(normalized.x, normalized.y))
    await existing.setSize(new LogicalSize(normalized.width, normalized.height))
    const state = frontendViewStates.get(platformId) ?? {
      platformId,
      title: platformName,
      canGoBack: false,
      canGoForward: false,
      loading: false,
      url,
    }
    frontendViewStates.set(platformId, state)
    await installPlatformViewHooks(platformId, storageId ?? rootPlatformIdForView(platformId)).catch(() => {})
    return state
  }

  const view = new Webview(getCurrentWindow(), label, {
    url,
    x: normalized.x,
    y: normalized.y,
    width: normalized.width,
    height: normalized.height,
    focus: false,
    dragDropEnabled: false,
    // Windows 前端 WebView 暂无独立 data directory 选项。复制平台用无痕
    // profile，避免同域平台继承原平台 Cookie/localStorage。
    incognito: shouldUseEphemeralFrontendStorage(platformId, storageId),
    ...(userAgent ? { userAgent } : {}),
  })

  frontendViews.set(platformId, view)
  await waitForWebviewCreated(view)
  await installPlatformViewHooks(platformId, storageId ?? rootPlatformIdForView(platformId)).catch(() => {})

  const state: PlatformViewState = {
    platformId,
    title: platformName,
    canGoBack: false,
    canGoForward: false,
    loading: false,
    url,
  }
  frontendViewStates.set(platformId, state)
  return state
}

export async function createPlatformView(
  platformId: string,
  platformName: string,
  url: string,
  bounds: ViewBounds,
  userAgent?: string,
  storageId?: string
) {
  if (shouldUseFrontendWebview()) {
    return createFrontendPlatformView(platformId, platformName, url, bounds, userAgent, storageId)
  }

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
  if (shouldUseFrontendWebview()) {
    await Promise.all([...frontendViews.entries()].map(([id, view]) => (
      id === platformId ? view.show() : view.hide()
    )))
    const view = await getFrontendView(platformId)
    if (view) {
      await view.show()
      await installPlatformViewHooks(platformId).catch(() => {})
      await view.setFocus().catch(() => {})
    }
    return
  }

  return invoke<void>('show_platform_view', { platformId })
}

export async function showPlatformViews(platformIds: string[]) {
  if (shouldUseFrontendWebview()) {
    const visibleIds = new Set(platformIds)
    await Promise.all([...frontendViews.entries()].map(([id, view]) => (
      visibleIds.has(id) ? view.show() : view.hide()
    )))
    return
  }

  return invoke<void>('show_platform_views', { platformIds })
}

export async function closePlatformView(platformId: string) {
  if (shouldUseFrontendWebview()) {
    const view = await getFrontendView(platformId)
    frontendViews.delete(platformId)
    frontendViewStates.delete(platformId)
    if (view) {
      await view.close()
    }
    return
  }

  return invoke<void>('close_platform_view', { platformId })
}

export async function hideAllPlatformViews() {
  if (shouldUseFrontendWebview()) {
    await Promise.all([...frontendViews.values()].map(view => view.hide()))
    return
  }

  return invoke<void>('hide_all_platform_views')
}

export async function setPlatformViewBounds(platformId: string, bounds: ViewBounds) {
  if (shouldUseFrontendWebview()) {
    const view = await getFrontendView(platformId)
    if (!view) return

    const normalized = normalizeBounds(bounds)
    await view.setPosition(new LogicalPosition(normalized.x, normalized.y))
    await view.setSize(new LogicalSize(normalized.width, normalized.height))
    return
  }

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

export async function fillPlatformInput(
  platformId: string,
  brand: string,
  text: string
) {
  return invoke<void>('fill_platform_input', { platformId, brand, text })
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
