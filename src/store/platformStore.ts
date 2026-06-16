import { create } from 'zustand'
import type { Platform, AppConfig, PromptTemplate } from '../types'
import { DEFAULT_PLATFORMS, DEFAULT_CONFIG } from '../config/defaults'

// 本地存储键名
const STORAGE_KEY_PLATFORMS = 'polychat-platforms'
const STORAGE_KEY_CONFIG = 'polychat-config'
const STORAGE_KEY_ACTIVE = 'polychat-active-platform'
const STORAGE_KEY_LAYOUT = 'polychat-layout-mode'
const STORAGE_KEY_SPLIT = 'polychat-split-platforms'
const STORAGE_KEY_TEMPLATES = 'polychat-prompt-templates'

export type LayoutMode = 'single' | 'split'
type JsonRecord = Record<string, unknown>

interface PlatformStore {
  // 平台列表
  platforms: Platform[]
  // 应用配置
  config: AppConfig
  // 当前激活的平台 ID
  activePlatformId: string | null
  // 布局模式：单屏 / 分屏
  layoutMode: LayoutMode
  // 分屏模式下可见的平台 ID 集合（有序=网格顺序）
  splitPlatformIds: string[]
  // Prompt 模板库
  promptTemplates: PromptTemplate[]
  // 是否显示添加平台弹窗
  showAddModal: boolean
  // 是否显示设置弹窗
  showSettingsModal: boolean

  // 操作
  setActivePlatform: (id: string) => void
  setLayoutMode: (mode: LayoutMode) => void
  toggleSplitPlatform: (id: string) => void
  setSplitPlatformIds: (ids: string[]) => void
  addPromptTemplate: (title: string, content: string) => void
  removePromptTemplate: (id: string) => void
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

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isLayoutMode(value: unknown): value is LayoutMode {
  return value === 'single' || value === 'split'
}

function normalizePlatform(value: unknown): Platform | null {
  if (!isRecord(value)) return null
  if (typeof value.id !== 'string' || typeof value.name !== 'string' || typeof value.url !== 'string') {
    return null
  }

  const iconType =
    value.iconType === 'emoji' || value.iconType === 'url' || value.iconType === 'favicon'
      ? value.iconType
      : 'favicon'

  return {
    id: value.id,
    name: value.name,
    url: value.url,
    icon: typeof value.icon === 'string' ? value.icon : value.url,
    iconType,
    enabled: typeof value.enabled === 'boolean' ? value.enabled : true,
    order: typeof value.order === 'number' && Number.isFinite(value.order) ? value.order : 0,
    description: typeof value.description === 'string' ? value.description : undefined,
    userAgent: typeof value.userAgent === 'string' ? value.userAgent : undefined,
    injectScript: typeof value.injectScript === 'string' ? value.injectScript : undefined,
  }
}

function normalizePlatforms(value: unknown): Platform[] {
  if (!Array.isArray(value)) return DEFAULT_PLATFORMS

  const normalized = value
    .map(normalizePlatform)
    .filter((platform): platform is Platform => Boolean(platform))

  return normalized.length > 0 ? normalized : DEFAULT_PLATFORMS
}

function normalizeConfig(value: unknown): AppConfig {
  return {
    ...DEFAULT_CONFIG,
    ...(isRecord(value) ? value : {}),
  }
}

function normalizeStringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []
}

function normalizePromptTemplates(value: unknown): PromptTemplate[] {
  if (!Array.isArray(value)) return []

  return value.flatMap(item => {
    if (!isRecord(item)) return []
    if (
      typeof item.id !== 'string' ||
      typeof item.title !== 'string' ||
      typeof item.content !== 'string' ||
      typeof item.createdAt !== 'number'
    ) {
      return []
    }

    return [{
      id: item.id,
      title: item.title,
      content: item.content,
      createdAt: item.createdAt,
    }]
  })
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
  const rawPlatforms = normalizePlatforms(loadFromStorage<unknown>(STORAGE_KEY_PLATFORMS, DEFAULT_PLATFORMS))
  const savedPlatforms = migratePlatforms(rawPlatforms)
  saveToStorage(STORAGE_KEY_PLATFORMS, savedPlatforms)
  const savedConfig = normalizeConfig(loadFromStorage<unknown>(STORAGE_KEY_CONFIG, DEFAULT_CONFIG))
  const rawActiveId = loadFromStorage<unknown>(STORAGE_KEY_ACTIVE, null)
  const savedActiveId = typeof rawActiveId === 'string' ? rawActiveId : null

  // 确定默认激活的平台
  let initialActiveId = savedActiveId
  if (!initialActiveId || !savedPlatforms.find(p => p.id === initialActiveId && p.enabled)) {
    initialActiveId = savedPlatforms.find(p => p.enabled)?.id ?? null
  }

  // 加载布局模式与分屏集合
  const rawLayout = loadFromStorage<unknown>(STORAGE_KEY_LAYOUT, 'single')
  const savedLayout = isLayoutMode(rawLayout) ? rawLayout : 'single'
  const enabledIdSet = new Set(savedPlatforms.filter(p => p.enabled).map(p => p.id))
  const rawSplit = normalizeStringArray(loadFromStorage<unknown>(STORAGE_KEY_SPLIT, []))
  let initialSplitIds = rawSplit.filter(id => enabledIdSet.has(id))
  if (initialSplitIds.length === 0 && initialActiveId) {
    initialSplitIds = [initialActiveId]
  }

  // 加载 Prompt 模板库
  const savedTemplates = normalizePromptTemplates(loadFromStorage<unknown>(STORAGE_KEY_TEMPLATES, []))

  return {
    platforms: savedPlatforms,
    config: savedConfig,
    activePlatformId: initialActiveId,
    layoutMode: savedLayout,
    splitPlatformIds: initialSplitIds,
    promptTemplates: savedTemplates,
    showAddModal: false,
    showSettingsModal: false,

    setActivePlatform: (id) => {
      set({ activePlatformId: id })
      saveToStorage(STORAGE_KEY_ACTIVE, id)
    },

    setLayoutMode: (mode) => {
      const { splitPlatformIds, activePlatformId } = get()
      // 切到分屏时若集合为空，用当前活跃平台 seed
      let nextSplit = splitPlatformIds
      if (mode === 'split' && nextSplit.length === 0 && activePlatformId) {
        nextSplit = [activePlatformId]
        set({ splitPlatformIds: nextSplit })
        saveToStorage(STORAGE_KEY_SPLIT, nextSplit)
      }
      set({ layoutMode: mode })
      saveToStorage(STORAGE_KEY_LAYOUT, mode)
    },

    toggleSplitPlatform: (id) => {
      const { splitPlatformIds } = get()
      let updated: string[]
      if (splitPlatformIds.includes(id)) {
        // 防止移除到空集合
        if (splitPlatformIds.length <= 1) return
        updated = splitPlatformIds.filter(x => x !== id)
      } else {
        updated = [...splitPlatformIds, id]
      }
      set({ splitPlatformIds: updated })
      saveToStorage(STORAGE_KEY_SPLIT, updated)
    },

    setSplitPlatformIds: (ids) => {
      set({ splitPlatformIds: ids })
      saveToStorage(STORAGE_KEY_SPLIT, ids)
    },

    addPromptTemplate: (title, content) => {
      const { promptTemplates } = get()
      const tpl: PromptTemplate = {
        id: `tpl_${Date.now()}`,
        title: title.trim() || '未命名模板',
        content,
        createdAt: Date.now(),
      }
      const updated = [...promptTemplates, tpl]
      set({ promptTemplates: updated })
      saveToStorage(STORAGE_KEY_TEMPLATES, updated)
    },

    removePromptTemplate: (id) => {
      const { promptTemplates } = get()
      const updated = promptTemplates.filter(t => t.id !== id)
      set({ promptTemplates: updated })
      saveToStorage(STORAGE_KEY_TEMPLATES, updated)
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

      // Windows 平台提示
      if (typeof window !== 'undefined' && /Windows/i.test(navigator.userAgent)) {
        const confirmed = window.confirm(
          'Windows 平台限制说明\n\n' +
          '由于 Windows WebView2 的技术限制，克隆平台使用无痕模式，关闭应用后登录状态将会丢失。\n\n' +
          '建议方案：\n' +
          '  • 使用平台自带的账号切换功能\n' +
          '  • 或在浏览器中打开进行多账号管理\n\n' +
          '是否继续创建克隆平台？'
        )
        if (!confirmed) return
      }

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
      const { platforms, activePlatformId, splitPlatformIds } = get()
      const updated = platforms.filter(p => p.id !== id)
      let newActiveId = activePlatformId
      if (activePlatformId === id) {
        newActiveId = updated.find(p => p.enabled)?.id ?? null
      }
      let newSplit = splitPlatformIds.filter(x => x !== id)
      if (newSplit.length === 0 && newActiveId) newSplit = [newActiveId]
      set({ platforms: updated, activePlatformId: newActiveId, splitPlatformIds: newSplit })
      saveToStorage(STORAGE_KEY_PLATFORMS, updated)
      saveToStorage(STORAGE_KEY_ACTIVE, newActiveId)
      saveToStorage(STORAGE_KEY_SPLIT, newSplit)
    },

    togglePlatform: (id) => {
      const { platforms, activePlatformId, splitPlatformIds } = get()
      const updated = platforms.map(p =>
        p.id === id ? { ...p, enabled: !p.enabled } : p
      )
      // 如果禁用的是当前激活的平台，切换到另一个启用的平台
      let newActiveId = activePlatformId
      const toggled = updated.find(p => p.id === id)
      if (id === activePlatformId && toggled && !toggled.enabled) {
        newActiveId = updated.find(p => p.enabled)?.id ?? null
      }
      // 禁用的平台同步移出分屏集合
      let newSplit = splitPlatformIds
      if (toggled && !toggled.enabled) {
        newSplit = splitPlatformIds.filter(x => x !== id)
        if (newSplit.length === 0 && newActiveId) newSplit = [newActiveId]
      }
      set({ platforms: updated, activePlatformId: newActiveId, splitPlatformIds: newSplit })
      saveToStorage(STORAGE_KEY_PLATFORMS, updated)
      saveToStorage(STORAGE_KEY_ACTIVE, newActiveId)
      saveToStorage(STORAGE_KEY_SPLIT, newSplit)
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
      const defaultActiveId = DEFAULT_PLATFORMS.find(p => p.enabled)?.id ?? null
      const defaultSplit = defaultActiveId ? [defaultActiveId] : []
      set({
        platforms: DEFAULT_PLATFORMS,
        config: DEFAULT_CONFIG,
        activePlatformId: defaultActiveId,
        layoutMode: 'single',
        splitPlatformIds: defaultSplit,
        promptTemplates: [],
      })
      saveToStorage(STORAGE_KEY_PLATFORMS, DEFAULT_PLATFORMS)
      saveToStorage(STORAGE_KEY_CONFIG, DEFAULT_CONFIG)
      saveToStorage(STORAGE_KEY_ACTIVE, defaultActiveId)
      saveToStorage(STORAGE_KEY_LAYOUT, 'single')
      saveToStorage(STORAGE_KEY_SPLIT, defaultSplit)
      saveToStorage(STORAGE_KEY_TEMPLATES, [])
    },
  }
})
