import React, { useRef, useEffect, useState, useCallback, useMemo } from 'react'
import type { Platform } from '../../types'
import {
  clearPlatformData,
  createPlatformView,
  navigate,
  closePlatformView,
  onOpenTabRequested,
  onPlatformStateChanged,
  onShortcut,
  openExternal,
  setPlatformViewBounds,
  showPlatformView,
  type ViewBounds,
  type PlatformViewState,
} from '../../runtime/desktop'
import { getFaviconCandidates } from '../../utils/platformIcons'
import './styles.css'

interface WebViewContainerProps {
  platform: Platform
  isActive: boolean
  onPlatformLoaded?: (id: string) => void
}

interface PlatformTab {
  id: string
  title: string
  url: string
  primary: boolean
}

const PlatformTitleIcon: React.FC<{ platform: Platform }> = ({ platform }) => {
  const [iconIndex, setIconIndex] = useState(0)
  const [iconError, setIconError] = useState(false)
  const [iconLoaded, setIconLoaded] = useState(false)

  const iconCandidates = useMemo(
    () => platform.iconType === 'favicon' ? getFaviconCandidates(platform.icon) : [],
    [platform.icon, platform.iconType]
  )

  useEffect(() => {
    setIconIndex(0)
    setIconError(false)
    setIconLoaded(false)
  }, [platform.icon, platform.iconType])

  useEffect(() => {
    if (platform.iconType !== 'favicon' || iconError || iconLoaded) return

    const timeoutId = window.setTimeout(() => {
      if (iconIndex < iconCandidates.length - 1) {
        setIconIndex(index => index + 1)
        setIconLoaded(false)
      } else {
        setIconError(true)
      }
    }, 1800)

    return () => window.clearTimeout(timeoutId)
  }, [iconCandidates.length, iconError, iconIndex, iconLoaded, platform.iconType])

  if (platform.iconType === 'emoji') {
    return <span className="webview-title-emoji">{platform.icon}</span>
  }

  if (platform.iconType !== 'favicon' || iconError) {
    return <span className="webview-title-fallback">{platform.name.charAt(0).toUpperCase()}</span>
  }

  return (
    <span className="webview-title-favicon">
      <span className="webview-title-fallback">{platform.name.charAt(0).toUpperCase()}</span>
      <img
        className={`webview-title-img ${iconLoaded ? 'loaded' : ''}`}
        src={iconCandidates[iconIndex]}
        alt={platform.name}
        onError={() => {
          if (iconIndex < iconCandidates.length - 1) {
            setIconIndex(index => index + 1)
            setIconLoaded(false)
          } else {
            setIconError(true)
          }
        }}
        onLoad={(e) => {
          const image = e.currentTarget
          if (image.naturalWidth > 0 && image.naturalHeight > 0) {
            setIconLoaded(true)
          } else if (iconIndex < iconCandidates.length - 1) {
            setIconIndex(index => index + 1)
            setIconLoaded(false)
          } else {
            setIconError(true)
          }
        }}
      />
    </span>
  )
}

const WebViewContainer: React.FC<WebViewContainerProps> = ({ platform, isActive, onPlatformLoaded }) => {
  const containerRef = useRef<HTMLDivElement | null>(null)
  const navbarRef = useRef<HTMLDivElement | null>(null)
  const wrapperRef = useRef<HTMLDivElement | null>(null)
  const createdViewIdsRef = useRef<Set<string>>(new Set())
  const readyViewIdsRef = useRef<Set<string>>(new Set())
  const recentTabRequestsRef = useRef<Map<string, number>>(new Map())
  const tabCounterRef = useRef(0)
  const primaryConfigRef = useRef({
    name: platform.name,
    url: platform.url,
    userAgent: platform.userAgent,
  })
  const [tabs, setTabs] = useState<PlatformTab[]>([
    { id: platform.id, title: platform.name, url: platform.url, primary: true },
  ])
  const [activeViewId, setActiveViewId] = useState(platform.id)
  const [states, setStates] = useState<Record<string, PlatformViewState>>({})

  // 用 ref 追踪 isActive，避免事件监听器闭包捕获旧值
  const isActiveRef = useRef(isActive)
  useEffect(() => { isActiveRef.current = isActive }, [isActive])

  const activeState = states[activeViewId]
  const activeTab = tabs.find(tab => tab.id === activeViewId) ?? tabs[0]
  const loading = activeState?.loading ?? true
  const title = activeState?.title || activeTab?.title || platform.name
  const canGoBack = activeState?.canGoBack ?? false
  const canGoForward = activeState?.canGoForward ?? false
  const currentUrl = activeState?.url || activeTab?.url || platform.url
  const showTabs = tabs.length > 1
  const canCloseActiveTab = showTabs && !activeTab?.primary

  const getBounds = useCallback((): ViewBounds | null => {
    const wrapper = wrapperRef.current
    if (!wrapper) return null

    const rect = wrapper.getBoundingClientRect()

    return {
      x: rect.left,
      y: rect.top,
      width: rect.width,
      height: rect.height,
      viewportHeight: window.innerHeight,
    }
  }, [])

  const getHiddenBounds = useCallback((): ViewBounds => ({
    x: -10000,
    y: -10000,
    width: 1,
    height: 1,
    viewportHeight: window.innerHeight,
  }), [])

  const getViewBounds = useCallback((viewId: string): ViewBounds | null => {
    const bounds = getBounds()
    if (!bounds) return null

    const loadingView = states[viewId]?.loading ?? true
    return loadingView && !readyViewIdsRef.current.has(viewId) ? getHiddenBounds() : bounds
  }, [getBounds, getHiddenBounds, states])

  const syncBounds = useCallback(async () => {
    if (createdViewIdsRef.current.size === 0) return
    try {
      await Promise.all(
        [...createdViewIdsRef.current].map(viewId => {
          const bounds = getViewBounds(viewId)
          return bounds ? setPlatformViewBounds(viewId, bounds) : Promise.resolve()
        })
      )
    } catch {
      // Tauri runtime unavailable in plain browser previews.
    }
  }, [getViewBounds])

  useEffect(() => {
    const previous = primaryConfigRef.current
    const urlChanged = previous.url !== platform.url
    const userAgentChanged = previous.userAgent !== platform.userAgent
    const nameChanged = previous.name !== platform.name

    if (!urlChanged && !userAgentChanged && !nameChanged) return

    primaryConfigRef.current = {
      name: platform.name,
      url: platform.url,
      userAgent: platform.userAgent,
    }

    setTabs(prev => prev.map(tab =>
      tab.primary ? { ...tab, title: platform.name, url: platform.url } : tab
    ))

    if (!urlChanged && !userAgentChanged) return

    if (createdViewIdsRef.current.has(platform.id)) {
      closePlatformView(platform.id).catch(() => {})
      createdViewIdsRef.current.delete(platform.id)
    }
    readyViewIdsRef.current.delete(platform.id)

    setStates(prev => ({
      ...prev,
      [platform.id]: {
        platformId: platform.id,
        title: platform.name,
        canGoBack: false,
        canGoForward: false,
        loading: true,
        url: platform.url,
      },
    }))
  }, [platform.id, platform.name, platform.url, platform.userAgent])

  const ensureView = useCallback((tab: PlatformTab) => {
    const bounds = getViewBounds(tab.id) ?? getBounds()
    if (!bounds || createdViewIdsRef.current.has(tab.id)) return

    createPlatformView(
      tab.id,
      tab.title,
      tab.url,
      bounds,
      // 不传默认值，让 Rust 端按操作系统选 UA（macOS=Safari，Win=Edge，Linux=Chrome）
      platform.userAgent || undefined,
      platform.id
    ).then(state => {
      createdViewIdsRef.current.add(tab.id)
      setStates(prev => ({ ...prev, [tab.id]: state }))
      if (isActiveRef.current && activeViewId === tab.id) {
        return showPlatformView(tab.id)
      }
    }).catch(() => {
      setStates(prev => ({
        ...prev,
        [tab.id]: {
          platformId: tab.id,
          title: tab.title,
          canGoBack: false,
          canGoForward: false,
          loading: false,
          url: tab.url,
        },
      }))
    })
  }, [activeViewId, getBounds, getViewBounds, platform.id, platform.userAgent])

  // 监听尺寸变化，native webview 会在平台首次激活时创建。
  useEffect(() => {
    let resizeObserver: ResizeObserver | null = null

    const observedElements = [
      containerRef.current,
      navbarRef.current,
      wrapperRef.current,
    ].filter((element): element is HTMLDivElement => Boolean(element))

    if (observedElements.length > 0) {
      resizeObserver = new ResizeObserver(() => {
        syncBounds()
      })
      observedElements.forEach(element => resizeObserver?.observe(element))
    }
    window.addEventListener('resize', syncBounds)

    return () => {
      resizeObserver?.disconnect()
      window.removeEventListener('resize', syncBounds)
    }
  }, [syncBounds])

  useEffect(() => {
    if (isActive) {
      if (activeTab) ensureView(activeTab)
      syncBounds()
      showPlatformView(activeViewId).catch(() => {})
    }
  }, [
    activeTab,
    activeViewId,
    ensureView,
    isActive,
    syncBounds,
  ])

  useEffect(() => {
    if (isActive) {
      syncBounds()
    }
  }, [activeViewId, isActive, loading, showTabs, syncBounds])

  useEffect(() => {
    let cleanup: (() => void) | undefined

    onPlatformStateChanged(state => {
      if (state.platformId !== platform.id && !state.platformId.startsWith(`${platform.id}__tab_`)) return

      const newTitle = state.title || platform.name
      if (!state.loading) {
        readyViewIdsRef.current.add(state.platformId)
        if (state.platformId === platform.id) {
          onPlatformLoaded?.(platform.id)
        }
      }
      setStates(prev => ({ ...prev, [state.platformId]: state }))
      setTabs(prev => prev.map(tab =>
        tab.id === state.platformId
          ? { ...tab, title: newTitle, url: state.url || tab.url }
          : tab
      ))

    }).then(unlisten => {
      cleanup = unlisten
    }).catch(() => {})

    return () => cleanup?.()
  }, [platform.id, platform.name, onPlatformLoaded])

  const createPlatformTab = useCallback((url: string, title?: string) => {
    tabCounterRef.current += 1
    const tabId = `${platform.id}__tab_${Date.now()}_${tabCounterRef.current}`
    const tabTitle = title || (() => {
      try {
        return new URL(url).hostname
      } catch {
        return '新标签页'
      }
    })()
    const tab: PlatformTab = {
      id: tabId,
      title: tabTitle,
      url,
      primary: false,
    }

    setTabs(prev => [...prev, tab])
    setActiveViewId(tabId)
    setStates(prev => ({
      ...prev,
      [tabId]: {
        platformId: tabId,
        title: tab.title,
        canGoBack: false,
        canGoForward: false,
        loading: true,
        url,
      },
    }))
  }, [platform.id])

  useEffect(() => {
    let cleanup: (() => void) | undefined

    onOpenTabRequested(request => {
      if (request.platformId !== platform.id) return

      const requestKey = `${request.openerViewId}:${request.url}`
      const now = Date.now()
      const lastRequestedAt = recentTabRequestsRef.current.get(requestKey) ?? 0
      if (now - lastRequestedAt < 800) return
      recentTabRequestsRef.current.set(requestKey, now)

      recentTabRequestsRef.current.forEach((timestamp, key) => {
        if (now - timestamp > 3000) {
          recentTabRequestsRef.current.delete(key)
        }
      })

      createPlatformTab(request.url)
    }).then(unlisten => {
      cleanup = unlisten
    }).catch(() => {})

    return () => cleanup?.()
  }, [createPlatformTab, platform.id])

  const handleNewTab = useCallback(() => {
    createPlatformTab(platform.url, platform.name)
  }, [createPlatformTab, platform.name, platform.url])

  // 重新加载
  const handleRefresh = useCallback(() => {
    setStates(prev => ({
      ...prev,
      [activeViewId]: {
        ...(prev[activeViewId] ?? {
          platformId: activeViewId,
          title,
          canGoBack,
          canGoForward,
          url: currentUrl,
        }),
        loading: true,
      },
    }))
    navigate(activeViewId, 'reload').catch(() => {
      setStates(prev => ({
        ...prev,
        [activeViewId]: { ...prev[activeViewId], loading: false },
      }))
    })
  }, [activeViewId, canGoBack, canGoForward, currentUrl, title])

  // 后退
  const handleGoBack = useCallback(() => {
    if (!canGoBack) return
    navigate(activeViewId, 'back').catch(() => {})
  }, [activeViewId, canGoBack])

  // 前进
  const handleGoForward = useCallback(() => {
    if (!canGoForward) return
    navigate(activeViewId, 'forward').catch(() => {})
  }, [activeViewId, canGoForward])

  // 在系统浏览器中打开当前页面
  const handleOpenInBrowser = useCallback(() => {
    openExternal(currentUrl || platform.url).catch(() => {
      window.open(currentUrl || platform.url, '_blank', 'noopener,noreferrer')
    })
  }, [currentUrl, platform.url])

  // 清除 session 并重新加载
  const handleClearSession = useCallback(async () => {
    await clearPlatformData(platform.id).catch(() => {})
    handleRefresh()
  }, [platform.id, handleRefresh])

  const handleSelectTab = useCallback((tabId: string) => {
    setActiveViewId(tabId)
  }, [])

  const switchTabByOffset = useCallback((offset: number) => {
    if (tabs.length <= 1) return
    const activeIndex = tabs.findIndex(tab => tab.id === activeViewId)
    const startIndex = activeIndex >= 0 ? activeIndex : 0
    const nextIndex = (startIndex + offset + tabs.length) % tabs.length
    setActiveViewId(tabs[nextIndex].id)
  }, [activeViewId, tabs])

  useEffect(() => {
    if (!isActive) return

    const handler = (event: KeyboardEvent) => {
      const isMod = event.metaKey || event.ctrlKey
      if (!isMod || event.key !== 'Tab') return

      event.preventDefault()
      switchTabByOffset(event.shiftKey ? -1 : 1)
    }

    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [isActive, switchTabByOffset])

  // 焦点在原生 WebView 中时，cmd/ctrl+Tab 由注入脚本通过 Tauri 事件桥接过来。
  // 用 ref 持有最新依赖，订阅只挂载一次，避免重订阅造成的丢键。
  const tabShortcutRef = useRef({ isActive, switchTabByOffset })
  tabShortcutRef.current = { isActive, switchTabByOffset }

  useEffect(() => {
    const unlistenPromise = onShortcut(event => {
      if (event.action !== 'switch-tab' || typeof event.offset !== 'number') return
      const { isActive: active, switchTabByOffset: switchTab } = tabShortcutRef.current
      if (!active) return
      switchTab(event.offset)
    })
    return () => {
      unlistenPromise.then(unlisten => unlisten()).catch(() => {})
    }
  }, [])

  const handleCloseTab = useCallback((tabId: string) => {
    const tab = tabs.find(item => item.id === tabId)
    if (!tab || tab.primary) return

    closePlatformView(tabId).catch(() => {})
    createdViewIdsRef.current.delete(tabId)
    readyViewIdsRef.current.delete(tabId)
    setStates(prev => {
      const next = { ...prev }
      delete next[tabId]
      return next
    })
    setTabs(prev => {
      const next = prev.filter(item => item.id !== tabId)
      if (activeViewId === tabId) {
        setActiveViewId(next[next.length - 1]?.id ?? platform.id)
      }
      return next
    })
  }, [activeViewId, platform.id, tabs])

  const handleCloseExtraTabs = useCallback(() => {
    const extraTabs = tabs.filter(tab => !tab.primary)
    if (extraTabs.length === 0) return

    extraTabs.forEach(tab => {
      closePlatformView(tab.id).catch(() => {})
      createdViewIdsRef.current.delete(tab.id)
      readyViewIdsRef.current.delete(tab.id)
    })

    setStates(prev => {
      const next = { ...prev }
      extraTabs.forEach(tab => {
        delete next[tab.id]
      })
      return next
    })
    setTabs(prev => prev.filter(tab => tab.primary))
    setActiveViewId(platform.id)
  }, [platform.id, tabs])

  const handleCloseActiveTab = useCallback(() => {
    if (!canCloseActiveTab) return
    handleCloseTab(activeViewId)
  }, [activeViewId, canCloseActiveTab, handleCloseTab])

  return (
    <div ref={containerRef} className={`webview-container ${isActive ? 'active' : ''}`}>
      {/* 导航栏 */}
      <div ref={navbarRef} className="webview-navbar">
        <div className="webview-nav-buttons">
          <button
            className={`btn-icon webview-nav-btn ${!canGoBack ? 'disabled' : ''}`}
            onClick={handleGoBack}
            disabled={!canGoBack}
            title="后退"
          >
            ◀
          </button>
          <button
            className={`btn-icon webview-nav-btn ${!canGoForward ? 'disabled' : ''}`}
            onClick={handleGoForward}
            disabled={!canGoForward}
            title="前进"
          >
            ▶
          </button>
          <button
            className="btn-icon webview-nav-btn"
            onClick={handleRefresh}
            title="刷新"
          >
            {loading ? '⏳' : '🔄'}
          </button>
        </div>

        <div className="webview-title" title={title}>
          <span className="webview-title-icon">
            <PlatformTitleIcon platform={platform} />
          </span>
          <span className="webview-title-text">{title}</span>
          {loading && <span className="webview-loading-dot">●</span>}
        </div>

        <div className="webview-actions">
          {showTabs && (
            <>
              <select
                className="webview-tab-select"
                value={activeViewId}
                onChange={(event) => handleSelectTab(event.target.value)}
                title="切换标签页"
              >
                {tabs.map(tab => (
                  <option key={tab.id} value={tab.id}>
                    {tab.primary ? platform.name : tab.title}
                  </option>
                ))}
              </select>
              <button
                className={`btn-icon webview-nav-btn ${!canCloseActiveTab ? 'disabled' : ''}`}
                onClick={handleCloseActiveTab}
                disabled={!canCloseActiveTab}
                title="关闭当前非平台网页"
              >
                ×
              </button>
              <button
                className="btn-icon webview-nav-btn"
                onClick={handleCloseExtraTabs}
                title="关闭所有非平台网页"
              >
                ⊠
              </button>
            </>
          )}
          <button
            className="btn-icon webview-nav-btn"
            onClick={handleOpenInBrowser}
            title="在系统浏览器中打开"
          >
            ↗
          </button>
          <button
            className="btn-icon webview-nav-btn"
            onClick={handleClearSession}
            title="清除登录状态并刷新"
          >
            🗑️
          </button>
        </div>
      </div>

      {/* 多 tab 时显示的精简 tab 条 */}
      {showTabs && (
        <div className="webview-tab-bar">
          {tabs.map(tab => (
            <button
              key={tab.id}
              className={`webview-tab${tab.id === activeViewId ? ' active' : ''}${tab.primary ? ' primary' : ''}`}
              onClick={() => handleSelectTab(tab.id)}
              title={tab.primary ? platform.name : tab.title}
            >
              <span className="webview-tab-title">
                {tab.primary ? platform.name : tab.title}
              </span>
              {!tab.primary && (
                <span
                  className="webview-tab-close"
                  role="button"
                  aria-label="关闭"
                  onClick={(event) => {
                    event.stopPropagation()
                    handleCloseTab(tab.id)
                  }}
                >
                  ×
                </span>
              )}
            </button>
          ))}
          <div className="webview-tab-bar-actions">
            <button
              className="btn-icon webview-tab-bar-btn"
              onClick={handleCloseExtraTabs}
              title="关闭所有非平台网页"
            >
              ⊠
            </button>
          </div>
        </div>
      )}

      {/* webview */}
      <div ref={wrapperRef} className="webview-wrapper">
        <div className="webview-placeholder">
          {loading ? '正在加载...' : '页面由系统 WebView 渲染'}
        </div>
      </div>
    </div>
  )
}

export default WebViewContainer
