import React, { useEffect, useMemo, useState } from 'react'
import type { Platform } from '../../types'
import type { LayoutMode } from '../../store/platformStore'
import { getFaviconCandidates } from '../../utils/platformIcons'
import './styles.css'

interface SidebarProps {
  platforms: Platform[]
  activeId: string | null
  loadedIds: Set<string>
  collapsed: boolean
  layoutMode: LayoutMode
  splitEnabled: boolean
  splitIds: string[]
  onSelect: (id: string) => void
  onToggleLayout: () => void
  onToggleSplit: (id: string) => void
  onToggleCollapse: () => void
  onSettingsClick: () => void
}

const PlatformLetterIcon: React.FC<{ platform: Platform; size?: 'normal' | 'compact' }> = ({
  platform,
  size = 'normal',
}) => (
  <span
    className={`sidebar-item-icon sidebar-item-letter ${
      size === 'compact' ? 'sidebar-item-letter-compact' : ''
    }`}
  >
    {platform.name.charAt(0).toUpperCase()}
  </span>
)

const PlatformIcon: React.FC<{ platform: Platform; size?: 'normal' | 'compact' }> = ({
  platform,
  size = 'normal',
}) => {
  const [faviconError, setFaviconError] = useState(false)
  const [faviconIndex, setFaviconIndex] = useState(0)
  const [faviconLoaded, setFaviconLoaded] = useState(false)

  const faviconCandidates = useMemo(
    () => platform.iconType === 'favicon' ? getFaviconCandidates(platform.icon) : [],
    [platform.icon, platform.iconType]
  )

  useEffect(() => {
    setFaviconError(false)
    setFaviconIndex(0)
    setFaviconLoaded(false)
  }, [platform.icon, platform.iconType])

  useEffect(() => {
    if (platform.iconType !== 'favicon' || faviconError || faviconLoaded) return

    const timeoutId = window.setTimeout(() => {
      if (faviconIndex < faviconCandidates.length - 1) {
        setFaviconIndex(index => index + 1)
        setFaviconLoaded(false)
      } else {
        setFaviconError(true)
      }
    }, 1800)

    return () => window.clearTimeout(timeoutId)
  }, [faviconCandidates.length, faviconError, faviconIndex, faviconLoaded, platform.iconType])

  if (platform.iconType === 'emoji') {
    return (
      <span className={size === 'compact' ? 'sidebar-item-icon sidebar-item-icon-compact' : 'sidebar-item-icon'}>
        {platform.icon}
      </span>
    )
  }

  if (platform.iconType === 'favicon' && !faviconError) {
    return (
      <span className={`sidebar-item-favicon ${size === 'compact' ? 'sidebar-item-favicon-compact' : ''}`}>
        <span className="sidebar-item-favicon-fallback">
          {platform.name.charAt(0).toUpperCase()}
        </span>
        <img
          className={`sidebar-item-img sidebar-item-img-overlay ${size === 'compact' ? 'sidebar-item-img-compact' : ''} ${
            faviconLoaded ? 'loaded' : ''
          }`}
          src={faviconCandidates[faviconIndex]}
          alt={platform.name}
          onError={() => {
            if (faviconIndex < faviconCandidates.length - 1) {
              setFaviconIndex(index => index + 1)
              setFaviconLoaded(false)
            } else {
              setFaviconError(true)
            }
          }}
          onLoad={(e) => {
            const image = e.currentTarget
            if (image.naturalWidth > 0 && image.naturalHeight > 0) {
              setFaviconLoaded(true)
            } else if (faviconIndex < faviconCandidates.length - 1) {
              setFaviconIndex(index => index + 1)
              setFaviconLoaded(false)
            } else {
              setFaviconError(true)
            }
          }}
        />
      </span>
    )
  }

  if (platform.iconType === 'url' && !faviconError) {
    return (
      <img
        className={`sidebar-item-img ${size === 'compact' ? 'sidebar-item-img-compact' : ''}`}
        src={platform.icon}
        alt={platform.name}
        onError={() => setFaviconError(true)}
      />
    )
  }

  // 回退：显示首字母
  return <PlatformLetterIcon platform={platform} size={size} />
}

const PlatformIconWithStatus: React.FC<{
  platform: Platform
  loaded: boolean
  size?: 'normal' | 'compact'
}> = ({ platform, loaded, size = 'normal' }) => (
  <span className={`sidebar-item-icon-wrap ${size === 'compact' ? 'sidebar-item-icon-wrap-compact' : ''}`}>
    <PlatformIcon platform={platform} size={size} />
    {loaded && <span className="sidebar-item-loaded-dot" />}
  </span>
)

const Sidebar: React.FC<SidebarProps> = ({
  platforms,
  activeId,
  loadedIds,
  collapsed,
  layoutMode,
  splitEnabled,
  splitIds,
  onSelect,
  onToggleLayout,
  onToggleSplit,
  onToggleCollapse,
  onSettingsClick,
}) => {
  const [dragIndex, setDragIndex] = useState<number | null>(null)
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null)

  const handleDragStart = (index: number) => {
    setDragIndex(index)
  }

  const handleDragOver = (e: React.DragEvent, index: number) => {
    e.preventDefault()
    setDragOverIndex(index)
  }

  const handleDragEnd = () => {
    setDragIndex(null)
    setDragOverIndex(null)
  }

  // 快捷键提示
  const getShortcut = (index: number) => {
    if (index < 9 && !collapsed) {
      return `⌘${index + 1}`
    }
    return undefined
  }

  return (
    <aside className={`sidebar ${collapsed ? 'collapsed' : ''}`}>
      {/* 标题栏 */}
      <div className="sidebar-header">
        {!collapsed && (
          <div className="sidebar-title">
            <img className="sidebar-logo" src="/icon_64.png" alt="PolyChat" />
            <span>PolyChat</span>
          </div>
        )}
        <button
          className="sidebar-toggle"
          onClick={onToggleCollapse}
          title={collapsed ? '展开侧边栏' : '折叠侧边栏'}
          aria-label={collapsed ? '展开侧边栏' : '折叠侧边栏'}
        >
          <svg
            className={`sidebar-toggle-icon ${collapsed ? 'is-collapsed' : ''}`}
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <polyline points="15 18 9 12 15 6" />
          </svg>
        </button>
      </div>

      {/* 搜索/筛选 - 仅展开时显示 */}
      {!collapsed && platforms.length > 5 && (
        <div className="sidebar-search">
          <input
            className="input sidebar-search-input"
            type="text"
            placeholder="搜索平台..."
            onInput={(e) => {
              // 简单的过滤功能（通过 CSS 类控制显示/隐藏）
              const query = (e.target as HTMLInputElement).value.toLowerCase()
              const items = document.querySelectorAll('.sidebar-item')
              items.forEach((item) => {
                const name = item.getAttribute('data-name')?.toLowerCase() || ''
                if (query && !name.includes(query)) {
                  item.classList.add('sidebar-item-hidden')
                } else {
                  item.classList.remove('sidebar-item-hidden')
                }
              })
            }}
          />
        </div>
      )}

      {/* 布局切换栏 */}
      {splitEnabled && (
      <div className={`sidebar-layout-switch ${collapsed ? 'is-collapsed' : ''}`}>
        {collapsed ? (
          <button
            className={`sidebar-layout-icon ${layoutMode === 'split' ? 'active' : ''}`}
            onClick={onToggleLayout}
            title={layoutMode === 'split' ? '切换为单屏' : '切换为分屏'}
            aria-label={layoutMode === 'split' ? '切换为单屏' : '切换为分屏'}
          >
            <span>{layoutMode === 'split' ? '▦' : '▢'}</span>
          </button>
        ) : (
          <div className="sidebar-layout-seg">
            <button
              className={`sidebar-layout-seg-btn ${layoutMode === 'single' ? 'active' : ''}`}
              onClick={() => { if (layoutMode !== 'single') onToggleLayout() }}
            >
              单屏
            </button>
            <button
              className={`sidebar-layout-seg-btn ${layoutMode === 'split' ? 'active' : ''}`}
              onClick={() => { if (layoutMode !== 'split') onToggleLayout() }}
            >
              分屏
            </button>
          </div>
        )}
      </div>
      )}

      {/* 平台列表 */}
      <nav className="sidebar-nav">
        {platforms.length === 0 && !collapsed && (
          <div className="sidebar-empty">
            <p>暂无启用平台</p>
            <p className="sidebar-empty-hint">点击下方 + 按钮添加</p>
          </div>
        )}

        {collapsed ? (
          /* 折叠模式：仅显示图标 */
          <div className="sidebar-item-group">
            {platforms.map((p, index) => (
              <button
                key={p.id}
                className={`sidebar-item sidebar-item-compact ${
                  (layoutMode === 'split' ? splitIds.includes(p.id) : p.id === activeId) ? 'active' : ''
                }`}
                onClick={() => layoutMode === 'split' ? onToggleSplit(p.id) : onSelect(p.id)}
                title={p.name}
                data-name={p.name}
              >
                <PlatformIconWithStatus platform={p} loaded={loadedIds.has(p.id)} size="compact" />
              </button>
            ))}
          </div>
        ) : (
          /* 展开模式 */
          <div className="sidebar-item-group">
            {platforms.map((p, index) => {
              const inSplit = splitIds.includes(p.id)
              const itemActive = layoutMode === 'split' ? inSplit : p.id === activeId
              return (
              <div
                key={p.id}
                className={`sidebar-item-wrapper ${
                  dragOverIndex === index ? 'drag-over' : ''
                }`}
                draggable
                onDragStart={() => handleDragStart(index)}
                onDragOver={(e) => handleDragOver(e, index)}
                onDragEnd={handleDragEnd}
              >
                <button
                  className={`sidebar-item ${itemActive ? 'active' : ''} ${
                    dragIndex === index ? 'dragging' : ''
                  }`}
                  onClick={() => layoutMode === 'split' ? onToggleSplit(p.id) : onSelect(p.id)}
                  data-name={p.name}
                >
                  <PlatformIconWithStatus platform={p} loaded={loadedIds.has(p.id)} />
                  <span className="sidebar-item-name">{p.name}</span>
                  {layoutMode === 'split' ? (
                    <span className={`sidebar-item-check ${inSplit ? 'checked' : ''}`}>
                      {inSplit ? '✓' : ''}
                    </span>
                  ) : (
                    getShortcut(index) && (
                      <span className="sidebar-item-shortcut">{getShortcut(index)}</span>
                    )
                  )}
                </button>
              </div>
              )
            })}
          </div>
        )}
      </nav>

      {/* 底部操作栏 */}
      <div className="sidebar-footer">
        {!collapsed && (
          <>
            <button className="sidebar-footer-btn" onClick={onSettingsClick} title="设置">
              <span>⚙️</span>
              <span>设置</span>
            </button>
          </>
        )}
        {collapsed && (
          <>
            <button className="sidebar-footer-btn sidebar-footer-btn-compact" onClick={onSettingsClick} title="设置">
              <span>⚙️</span>
            </button>
          </>
        )}
      </div>
    </aside>
  )
}

export default Sidebar
