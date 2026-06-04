import React, { useState, useEffect, useCallback, useRef } from 'react'
import Sidebar from './components/Sidebar'
import WebViewContainer from './components/WebViewContainer'
import AddPlatformModal from './components/AddPlatformModal'
import SettingsModal from './components/SettingsModal'
import BroadcastInput from './components/BroadcastInput'
import { usePlatformStore } from './store/platformStore'
import { hideAllPlatformViews, onDownloadFinished, onShortcut, openExternal, quitApp, showPlatformViews, switchConversation } from './runtime/desktop'

interface DownloadToast {
  id: number
  filename: string
  path: string | null
  success: boolean
}

const App: React.FC = () => {
  const {
    platforms,
    activePlatformId,
    layoutMode,
    splitPlatformIds,
    config,
    setActivePlatform,
    setLayoutMode,
    toggleSplitPlatform,
    showAddModal,
    showSettingsModal,
    setShowSettingsModal,
  } = usePlatformStore()

  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)
  const [loadedPlatformIds, setLoadedPlatformIds] = useState<Set<string>>(() => new Set())
  const [downloadToasts, setDownloadToasts] = useState<DownloadToast[]>([])

  // 启用的平台列表
  const enabledPlatforms = platforms
    .filter(p => p.enabled)
    .sort((a, b) => a.order - b.order)

  // 当前可见的平台 ID 集合：分屏取 splitPlatformIds，单屏取活跃平台
  const enabledIdSet = new Set(enabledPlatforms.map(p => p.id))
  const modalOpen = showAddModal || showSettingsModal
  // 分屏功能开关（设置页控制），关闭时强制单屏
  const splitEnabled = config.enableSplitView ?? false
  const effectiveLayoutMode = splitEnabled ? layoutMode : 'single'
  const visibleIds = (
    effectiveLayoutMode === 'split'
      ? splitPlatformIds.filter(id => enabledIdSet.has(id))
      : (activePlatformId && enabledIdSet.has(activePlatformId) ? [activePlatformId] : [])
  )

  const handleToggleLayout = useCallback(() => {
    setLayoutMode(layoutMode === 'split' ? 'single' : 'split')
  }, [layoutMode, setLayoutMode])

  // 点击侧边栏平台
  const handleSelectPlatform = useCallback((id: string) => {
    setActivePlatform(id)
  }, [setActivePlatform])

  const handlePlatformLoaded = useCallback((id: string) => {
    setLoadedPlatformIds(prev => {
      if (prev.has(id)) return prev
      const next = new Set(prev)
      next.add(id)
      return next
    })
  }, [])

  // 切换侧边栏折叠
  const toggleSidebar = useCallback(() => {
    setSidebarCollapsed(prev => !prev)
  }, [])

  // 统一可见性控制：modal 打开或无可见平台时全部隐藏；
  // 分屏模式下统一调 showPlatformViews 同时显示多个 view（单屏由容器内部处理）。
  const visibleKey = visibleIds.join('|')
  useEffect(() => {
    if (modalOpen || visibleIds.length === 0) {
      hideAllPlatformViews().catch(() => {})
      return
    }
    if (effectiveLayoutMode === 'split') {
      showPlatformViews(visibleIds).catch(() => {})
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visibleKey, effectiveLayoutMode, modalOpen])

  // 键盘快捷键: Ctrl/Cmd + 数字键切换平台，Ctrl/Cmd + Q 退出应用
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const isMod = e.ctrlKey || e.metaKey
      if (!isMod) return

      if (e.key.toLowerCase() === 'q') {
        e.preventDefault()
        quitApp().catch(() => {})
        return
      }

      if (e.shiftKey && (e.code === 'BracketLeft' || e.code === 'BracketRight')) {
        if (!activePlatformId) return
        e.preventDefault()
        switchConversation(activePlatformId, e.code === 'BracketRight' ? 1 : -1).catch(() => {})
        return
      }

      // Ctrl/Cmd + 1-9 切换平台
      const num = parseInt(e.key)
      if (num >= 1 && num <= 9 && num <= enabledPlatforms.length) {
        e.preventDefault()
        handleSelectPlatform(enabledPlatforms[num - 1].id)
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [activePlatformId, enabledPlatforms, handleSelectPlatform])

  // 监听原生下载完成事件，弹出 toast 提示
  useEffect(() => {
    let counter = 0
    const unlistenPromise = onDownloadFinished(event => {
      const id = ++counter
      const filename =
        event.filename ||
        (event.path ? event.path.split(/[\\/]/).pop() : null) ||
        '下载文件'
      setDownloadToasts(prev => [...prev, { id, filename, path: event.path, success: event.success }])
      window.setTimeout(() => {
        setDownloadToasts(prev => prev.filter(t => t.id !== id))
      }, 5000)
    })
    return () => {
      unlistenPromise.then(unlisten => unlisten()).catch(() => {})
    }
  }, [])

  const handleRevealDownload = useCallback((path: string | null) => {
    if (!path) return
    // 用 file:// URL 让系统打开所在文件夹（macOS Finder 会高亮该文件）
    const parent = path.replace(/[^/\\]+$/, '')
    openExternal(`file://${parent}`).catch(() => {})
  }, [])

  const dismissDownloadToast = useCallback((id: number) => {
    setDownloadToasts(prev => prev.filter(t => t.id !== id))
  }, [])

  // 当焦点在原生 WebView 中时，window 的 keydown 不会触发，
  // 通过注入到 WebView 的脚本经 Tauri 事件桥接回来处理快捷键。
  // 用 ref 持有最新值，订阅只在挂载时注册一次，避免 store 更新时反复 unlisten/relisten
  // 产生空窗期导致连按快捷键丢键。
  const shortcutCtxRef = useRef({ activePlatformId, enabledPlatforms, handleSelectPlatform })
  shortcutCtxRef.current = { activePlatformId, enabledPlatforms, handleSelectPlatform }

  useEffect(() => {
    const unlistenPromise = onShortcut(event => {
      if (event.action === 'switch-platform') {
        const idx = event.index ?? 0
        const { enabledPlatforms: list, handleSelectPlatform: select } = shortcutCtxRef.current
        if (idx >= 1 && idx <= list.length) {
          select(list[idx - 1].id)
        }
      } else if (event.action === 'switch-conversation') {
        const { activePlatformId: platformId } = shortcutCtxRef.current
        if (platformId && typeof event.offset === 'number') {
          switchConversation(platformId, event.offset).catch(() => {})
        }
      }
    })
    return () => {
      unlistenPromise.then(unlisten => unlisten()).catch(() => {})
    }
  }, [])

  return (
    <div
      className={`app-container${sidebarCollapsed ? ' sidebar-is-collapsed' : ''}`}
    >
      {/* 侧边栏 */}
      <Sidebar
        platforms={enabledPlatforms}
        activeId={activePlatformId}
        loadedIds={loadedPlatformIds}
        collapsed={sidebarCollapsed}
        layoutMode={effectiveLayoutMode}
        splitEnabled={splitEnabled}
        splitIds={splitPlatformIds}
        onSelect={handleSelectPlatform}
        onToggleLayout={handleToggleLayout}
        onToggleSplit={toggleSplitPlatform}
        onToggleCollapse={toggleSidebar}
        onSettingsClick={() => setShowSettingsModal(true)}
      />

      {/* 主内容区 */}
      <main className={`main-content layout-${effectiveLayoutMode} cols-${Math.min(visibleIds.length, 4)}`}>
        {/* 所有平台始终渲染，用 CSS 控制显隐（切换时 webview 不被销毁） */}
        {enabledPlatforms.length > 0 ? (
          <>
            <div className="webview-grid">
              {enabledPlatforms.map(p => (
                <WebViewContainer
                  key={p.id}
                  platform={p}
                  isActive={visibleIds.includes(p.id) && !modalOpen}
                  multiVisible={effectiveLayoutMode === 'split'}
                  onPlatformLoaded={handlePlatformLoaded}
                />
              ))}
            </div>
            {effectiveLayoutMode === 'split' && (
              <BroadcastInput visibleIds={visibleIds} platforms={enabledPlatforms} />
            )}
          </>
        ) : (
          <div className="empty-state">
            <div className="empty-state-icon">🤖</div>
            <h2>欢迎使用 PolyChat</h2>
            <p>请从左侧选择一个 AI 平台开始对话</p>
            <p className="empty-state-hint">
              提示：使用 Ctrl/Cmd + 数字键快速切换平台
            </p>
          </div>
        )}
      </main>

      {/* 弹窗 */}
      {showAddModal && <AddPlatformModal />}
      {showSettingsModal && <SettingsModal />}

      {/* 下载完成 toast */}
      {downloadToasts.length > 0 && (
        <div className="download-toasts">
          {downloadToasts.map(toast => (
            <div
              key={toast.id}
              className={`download-toast ${toast.success ? 'is-success' : 'is-error'}`}
            >
              <div className="download-toast-body">
                <span className="download-toast-icon">{toast.success ? '⬇' : '⚠'}</span>
                <div className="download-toast-text">
                  <span className="download-toast-title">
                    {toast.success ? '下载完成' : '下载失败'}
                  </span>
                  <span className="download-toast-filename" title={toast.path ?? toast.filename}>
                    {toast.filename}
                  </span>
                </div>
              </div>
              <div className="download-toast-actions">
                {toast.success && toast.path && (
                  <button
                    type="button"
                    className="download-toast-btn"
                    onClick={() => handleRevealDownload(toast.path)}
                  >
                    打开文件夹
                  </button>
                )}
                <button
                  type="button"
                  className="download-toast-close"
                  onClick={() => dismissDownloadToast(toast.id)}
                  aria-label="关闭"
                >
                  ✕
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

export default App
