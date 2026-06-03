import { create } from 'zustand'
import type { Platform, AppConfig } from '../types'
import { DEFAULT_PLATFORMS, DEFAULT_CONFIG } from '../config/defaults'

// 本地存储键名
const STORAGE_KEY_PLATFORMS = 'polychat-platforms'
const STORAGE_KEY_CONFIG = 'polychat-config'
const STORAGE_KEY_ACTIVE = 'polychat-active-platform'

interface PlatformStore {
  // 平台列表
  platforms: Platform[]
  // 应用配置
  config: AppConfig
  // 当前激活的平台 ID
  activePlatformId: string | null
  // 是否显示添加平台弹窗
  showAddModal: boolean
  // 是否显示设置弹窗
  showSettingsModal: boolean

  // 操作
  setActivePlatform: (id: string) => void
  addPlatform: (platform: Platform) => void
  duplicatePlatform: (id: string) => void
  updatePlatform: (id: string, updates: Partial<Platform>) => void
  removePlatform: (id: string) => void
  togglePlatform: (id: string) => void
  reorderPlatforms: (fromId: string, toId: string) => void
  updateConfig: (updates: Partial<AppConfig>) => void
  setShowAddModal: (show: boolean) => void
  setShowSettingsModal: (show: boolean) => void
  resetToDefaults: () => void
}

// 从 localStorage 加载数据
function loadFromStorage<T>(key: string, defaultValue: T): T {
  try {
    const stored = localStorage.getItem(key)
    return stored ? JSON.parse(stored) : defaultValue
  } catch {
    return defaultValue
  }
}

// 保存到 localStorage
function saveToStorage<T>(key: string, value: T) {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {
    // 存储失败时静默处理
  }
}

// 将已存储的平台列表与最新 defaults 合并：
// 内置平台补齐 defaults 新增字段，但保留用户在设置中修改过的字段；
// 用户自定义的平台（含 duplicatePlatform 克隆出的分身）原样保留。
function migratePlatforms(stored: Platform[]): Platform[] {
  const defaultMap = new Map(DEFAULT_PLATFORMS.map(p => [p.id, p]))
  const merged = stored.map(p => {
    const def = defaultMap.get(p.id)
    if (!def) return p
    return {
      ...def,
      ...p,
    }
  })
  // 补充 defaults 中存储里没有的新平台
  const storedIds = new Set(stored.map(p => p.id))
  DEFAULT_PLATFORMS.forEach(def => {
    if (!storedIds.has(def.id)) merged.push(def)
  })
  return merged
}

export const usePlatformStore = create<PlatformStore>((set, get) => {
  // 加载持久化数据
  const rawPlatforms = loadFromStorage<Platform[]>(STORAGE_KEY_PLATFORMS, DEFAULT_PLATFORMS)
  const savedPlatforms = migratePlatforms(rawPlatforms)
  saveToStorage(STORAGE_KEY_PLATFORMS, savedPlatforms)
  const savedConfig = loadFromStorage<AppConfig>(STORAGE_KEY_CONFIG, DEFAULT_CONFIG)
  const savedActiveId = loadFromStorage<string | null>(STORAGE_KEY_ACTIVE, null)

  // 确定默认激活的平台
  let initialActiveId = savedActiveId
  if (!initialActiveId || !savedPlatforms.find(p => p.id === initialActiveId && p.enabled)) {
    initialActiveId = savedPlatforms.find(p => p.enabled)?.id ?? null
  }

  return {
    platforms: savedPlatforms,
    config: savedConfig,
    activePlatformId: initialActiveId,
    showAddModal: false,
    showSettingsModal: false,

    setActivePlatform: (id) => {
      set({ activePlatformId: id })
      saveToStorage(STORAGE_KEY_ACTIVE, id)
    },

    addPlatform: (platform) => {
      const { platforms } = get()
      const updated = [...platforms, platform]
      set({ platforms: updated, showAddModal: false })
      saveToStorage(STORAGE_KEY_PLATFORMS, updated)
    },

    duplicatePlatform: (id) => {
      const { platforms } = get()
      const source = platforms.find(p => p.id === id)
      if (!source) return

      const siblingCount = platforms.filter(p =>
        p.id === source.id || p.id.startsWith(`${source.id}__clone_`) || p.url === source.url
      ).length
      const cloneId = `${source.id}__clone_${Date.now()}`
      const clone: Platform = {
        ...source,
        id: cloneId,
        name: `${source.name} #${siblingCount + 1}`,
        enabled: true,
        order: Math.max(...platforms.map(p => p.order), -1) + 1,
        description: source.description
          ? `${source.description}（独立登录分身）`
          : '独立登录分身',
      }
      const updated = [...platforms, clone]
      set({ platforms: updated, activePlatformId: cloneId })
      saveToStorage(STORAGE_KEY_PLATFORMS, updated)
      saveToStorage(STORAGE_KEY_ACTIVE, cloneId)
    },

    updatePlatform: (id, updates) => {
      const { platforms } = get()
      const updated = platforms.map(p =>
        p.id === id ? { ...p, ...updates } : p
      )
      set({ platforms: updated })
      saveToStorage(STORAGE_KEY_PLATFORMS, updated)
    },

    removePlatform: (id) => {
      const { platforms, activePlatformId } = get()
      const updated = platforms.filter(p => p.id !== id)
      let newActiveId = activePlatformId
      if (activePlatformId === id) {
        newActiveId = updated.find(p => p.enabled)?.id ?? null
      }
      set({ platforms: updated, activePlatformId: newActiveId })
      saveToStorage(STORAGE_KEY_PLATFORMS, updated)
      saveToStorage(STORAGE_KEY_ACTIVE, newActiveId)
    },

    togglePlatform: (id) => {
      const { platforms, activePlatformId } = get()
      const updated = platforms.map(p =>
        p.id === id ? { ...p, enabled: !p.enabled } : p
      )
      // 如果禁用的是当前激活的平台，切换到另一个启用的平台
      let newActiveId = activePlatformId
      const toggled = updated.find(p => p.id === id)
      if (id === activePlatformId && toggled && !toggled.enabled) {
        newActiveId = updated.find(p => p.enabled)?.id ?? null
      }
      set({ platforms: updated, activePlatformId: newActiveId })
      saveToStorage(STORAGE_KEY_PLATFORMS, updated)
      saveToStorage(STORAGE_KEY_ACTIVE, newActiveId)
    },

    reorderPlatforms: (fromId, toId) => {
      const { platforms } = get()
      const updated = [...platforms].sort((a, b) => a.order - b.order)
      const fromIndex = updated.findIndex(p => p.id === fromId)
      const toIndex = updated.findIndex(p => p.id === toId)
      if (fromIndex === -1 || toIndex === -1 || fromIndex === toIndex) return

      const [moved] = updated.splice(fromIndex, 1)
      updated.splice(toIndex, 0, moved)
      // 重新分配 order
      const reordered = updated.map((p, i) => ({ ...p, order: i }))
      set({ platforms: reordered })
      saveToStorage(STORAGE_KEY_PLATFORMS, reordered)
    },

    updateConfig: (updates) => {
      const { config } = get()
      const updated = { ...config, ...updates }
      set({ config: updated })
      saveToStorage(STORAGE_KEY_CONFIG, updated)
    },

    setShowAddModal: (show) => {
      set({ showAddModal: show })
    },

    setShowSettingsModal: (show) => {
      set({ showSettingsModal: show })
    },

    resetToDefaults: () => {
      set({
        platforms: DEFAULT_PLATFORMS,
        config: DEFAULT_CONFIG,
        activePlatformId: DEFAULT_PLATFORMS.find(p => p.enabled)?.id ?? null,
      })
      saveToStorage(STORAGE_KEY_PLATFORMS, DEFAULT_PLATFORMS)
      saveToStorage(STORAGE_KEY_CONFIG, DEFAULT_CONFIG)
      saveToStorage(STORAGE_KEY_ACTIVE, DEFAULT_PLATFORMS.find(p => p.enabled)?.id ?? null)
    },
  }
})
