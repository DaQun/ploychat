import React, { useEffect, useMemo, useRef, useState } from 'react'
import { usePlatformStore } from '../../store/platformStore'
import type { Platform } from '../../types'
import { getFaviconCandidates, normalizeIconDomain } from '../../utils/platformIcons'
import './styles.css'

const SettingsPlatformIcon: React.FC<{ platform: Platform }> = ({ platform }) => {
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
    return <span className="platform-manage-emoji">{platform.icon}</span>
  }

  if (platform.iconType !== 'favicon' || iconError) {
    return <span className="platform-manage-fallback">{platform.name.charAt(0).toUpperCase()}</span>
  }

  return (
    <span className="platform-manage-favicon">
      <span className="platform-manage-fallback">{platform.name.charAt(0).toUpperCase()}</span>
      <img
        className={`platform-manage-img ${iconLoaded ? 'loaded' : ''}`}
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

const SettingsModal: React.FC = () => {
  const {
    platforms,
    config,
    updateConfig,
    updatePlatform,
    duplicatePlatform,
    removePlatform,
    togglePlatform,
    reorderPlatforms,
    setShowAddModal,
    setShowSettingsModal,
    resetToDefaults,
  } = usePlatformStore()

  const [dragId, setDragId] = useState<string | null>(null)
  const [dragOverId, setDragOverId] = useState<string | null>(null)
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editName, setEditName] = useState('')
  const [editUrl, setEditUrl] = useState('')
  const [editDescription, setEditDescription] = useState('')
  const [editError, setEditError] = useState('')
  const dragIdRef = useRef<string | null>(null)
  const dragOverIdRef = useRef<string | null>(null)

  const sortedPlatforms = [...platforms].sort((a, b) => a.order - b.order)

  const getPlatformIdAtPoint = (clientX: number, clientY: number) => {
    const element = document.elementFromPoint(clientX, clientY)
    return element?.closest<HTMLElement>('[data-platform-id]')?.dataset.platformId ?? null
  }

  const handleDragPointerDown = (e: React.PointerEvent, id: string) => {
    if (e.button !== 0) return
    e.preventDefault()
    e.stopPropagation()

    dragIdRef.current = id
    dragOverIdRef.current = id
    setDragId(id)
    setDragOverId(id)
    e.currentTarget.setPointerCapture(e.pointerId)
  }

  const handleDragPointerMove = (e: React.PointerEvent) => {
    if (!dragIdRef.current) return

    e.preventDefault()
    const overId = getPlatformIdAtPoint(e.clientX, e.clientY)
    if (overId && overId !== dragOverIdRef.current) {
      dragOverIdRef.current = overId
      setDragOverId(overId)
    }
  }

  const finishDrag = (e: React.PointerEvent) => {
    if (!dragIdRef.current) return

    e.preventDefault()
    const fromId = dragIdRef.current
    const toId = getPlatformIdAtPoint(e.clientX, e.clientY) ?? dragOverIdRef.current
    if (toId && fromId !== toId) {
      reorderPlatforms(fromId, toId)
    }

    dragIdRef.current = null
    dragOverIdRef.current = null
    setDragId(null)
    setDragOverId(null)
  }

  const cancelDrag = () => {
    dragIdRef.current = null
    dragOverIdRef.current = null
    setDragId(null)
    setDragOverId(null)
  }

  const handleClose = () => setShowSettingsModal(false)
  const handleAddPlatform = () => {
    setShowSettingsModal(false)
    setShowAddModal(true)
  }
  const handleOverlayClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) handleClose()
  }
  const handleUserAgentChange = (id: string, userAgent: string) => {
    updatePlatform(id, { userAgent: userAgent || undefined })
  }

  const startEditing = (platform: Platform) => {
    setEditingId(platform.id)
    setEditName(platform.name)
    setEditUrl(platform.url)
    setEditDescription(platform.description ?? '')
    setEditError('')
    setPendingDeleteId(null)
  }

  const cancelEditing = () => {
    setEditingId(null)
    setEditError('')
  }

  const saveEditing = (platform: Platform) => {
    const name = editName.trim()
    const url = editUrl.trim()
    if (!name) {
      setEditError('请输入平台名称')
      return
    }
    if (!url) {
      setEditError('请输入平台 URL')
      return
    }

    const finalUrl = url.startsWith('http') ? url : `https://${url}`
    let parsedHost: string
    try {
      parsedHost = new URL(finalUrl).hostname
    } catch {
      setEditError('请输入有效的 URL')
      return
    }

    const patch: Partial<Platform> = {
      name,
      url: finalUrl,
      description: editDescription.trim() || undefined,
    }

    // URL 变更时同步 favicon 域名（仅当当前用 favicon 取图标，避免覆盖自定义 emoji/URL 图标）
    if (platform.iconType === 'favicon' && finalUrl !== platform.url) {
      patch.icon = normalizeIconDomain(parsedHost)
    }

    updatePlatform(platform.id, patch)
    setEditingId(null)
    setEditError('')
  }

  useEffect(() => {
    if (!pendingDeleteId) return

    const timeoutId = window.setTimeout(() => {
      setPendingDeleteId(null)
    }, 3000)

    return () => window.clearTimeout(timeoutId)
  }, [pendingDeleteId])

  return (
    <div className="modal-overlay" onClick={handleOverlayClick}>
      <div className="modal settings-modal">
        <div className="modal-header">
          <h2>⚙️ 设置</h2>
          <button className="btn-icon modal-close" onClick={handleClose}>✕</button>
        </div>

        <div className="modal-body settings-body">
          {/* 常规设置 */}
          <section className="settings-section">
            <h3 className="settings-section-title">常规</h3>

            <div className="settings-item">
              <div className="settings-item-info">
                <span className="settings-item-label">记住上次打开的平台</span>
                <span className="settings-item-desc">启动时自动打开上次使用的平台</span>
              </div>
              <label className="toggle">
                <input
                  type="checkbox"
                  checked={config.rememberLastPlatform}
                  onChange={e => updateConfig({ rememberLastPlatform: e.target.checked })}
                />
                <span className="toggle-slider"></span>
              </label>
            </div>

            <div className="settings-item">
              <div className="settings-item-info">
                <span className="settings-item-label">启动时最小化到托盘</span>
                <span className="settings-item-desc">打开应用时自动最小化到系统托盘</span>
              </div>
              <label className="toggle">
                <input
                  type="checkbox"
                  checked={config.minimizeToTray}
                  onChange={e => updateConfig({ minimizeToTray: e.target.checked })}
                />
                <span className="toggle-slider"></span>
              </label>
            </div>

          </section>

          <section className="settings-section">
            <h3 className="settings-section-title">
              平台管理
              <span className="settings-count">{platforms.length} 个平台</span>
              <button
                type="button"
                className="btn settings-section-action"
                onClick={handleAddPlatform}
              >
                添加平台
              </button>
            </h3>
            <p className="settings-drag-hint">拖动 ⠿ 可调整顺序</p>

            <div className="platform-list">
              {sortedPlatforms.map((p) => (
                <div
                  key={p.id}
                  data-platform-id={p.id}
                  className={`platform-manage-item ${dragOverId === p.id ? 'drag-over' : ''} ${dragId === p.id ? 'dragging' : ''} ${editingId === p.id ? 'editing' : ''}`}
                >
                  <div className="platform-manage-row">
                    <span
                      className="platform-manage-drag"
                      title="拖拽排序"
                      onPointerDown={(e) => handleDragPointerDown(e, p.id)}
                      onPointerMove={handleDragPointerMove}
                      onPointerUp={finishDrag}
                      onPointerCancel={cancelDrag}
                    >
                      ⠿
                    </span>
                    <div className="platform-manage-info">
                      <span className="platform-manage-icon">
                        <SettingsPlatformIcon platform={p} />
                      </span>
                      <div className="platform-manage-details">
                        <span className="platform-manage-name">{p.name}</span>
                        <span className="platform-manage-url">{p.url}</span>
                      </div>
                    </div>
                    <div className="platform-manage-actions">
                      <label className="toggle toggle-sm">
                        <input
                          type="checkbox"
                          checked={p.enabled}
                          onChange={() => togglePlatform(p.id)}
                        />
                        <span className="toggle-slider"></span>
                      </label>
                      <button
                        type="button"
                        className={`btn-icon platform-manage-edit ${editingId === p.id ? 'is-active' : ''}`}
                        onClick={(event) => {
                          event.preventDefault()
                          event.stopPropagation()
                          if (editingId === p.id) {
                            cancelEditing()
                          } else {
                            startEditing(p)
                          }
                        }}
                        title={editingId === p.id ? '收起编辑' : '编辑'}
                      >
                        ✏️
                      </button>
                      <button
                        type="button"
                        className="btn-icon platform-manage-clone"
                        onClick={(event) => {
                          event.preventDefault()
                          event.stopPropagation()
                          duplicatePlatform(p.id)
                        }}
                        title="创建独立登录分身"
                      >
                        ⧉
                      </button>
                      <button
                        type="button"
                        className="btn-icon platform-manage-delete"
                        onClick={(event) => {
                          event.preventDefault()
                          event.stopPropagation()
                          if (pendingDeleteId === p.id) {
                            removePlatform(p.id)
                            setPendingDeleteId(null)
                          } else {
                            setPendingDeleteId(p.id)
                          }
                        }}
                        title={pendingDeleteId === p.id ? '确认删除' : '删除'}
                      >
                        {pendingDeleteId === p.id ? '确认' : '🗑️'}
                      </button>
                    </div>
                  </div>
                  {editingId === p.id && (
                    <form
                      className="platform-manage-edit-form"
                      onSubmit={(event) => {
                        event.preventDefault()
                        saveEditing(p)
                      }}
                    >
                      {editError && <div className="platform-manage-edit-error">{editError}</div>}
                      <label className="platform-manage-edit-field">
                        <span>名称</span>
                        <input
                          className="input"
                          type="text"
                          value={editName}
                          onChange={(event) => { setEditName(event.target.value); setEditError('') }}
                          autoFocus
                        />
                      </label>
                      <label className="platform-manage-edit-field">
                        <span>URL</span>
                        <input
                          className="input"
                          type="text"
                          value={editUrl}
                          onChange={(event) => { setEditUrl(event.target.value); setEditError('') }}
                          placeholder="https://example.com/"
                        />
                      </label>
                      <label className="platform-manage-edit-field">
                        <span>备注</span>
                        <input
                          className="input"
                          type="text"
                          value={editDescription}
                          onChange={(event) => setEditDescription(event.target.value)}
                          placeholder="可选"
                        />
                      </label>
                      <div className="platform-manage-edit-actions">
                        <button type="button" className="btn" onClick={cancelEditing}>取消</button>
                        <button type="submit" className="btn btn-primary">保存</button>
                      </div>
                    </form>
                  )}
                </div>
              ))}
            </div>
          </section>

          {/* 关于 */}
          <section className="settings-section">
            <h3 className="settings-section-title">关于</h3>
            <div className="about-info">
              <p>PolyChat v1.0.0</p>
              <p>多平台 AI 网页版客户端</p>
              <p className="about-hint">
                提示：所有 AI 平台的登录状态都保存在本地，关闭应用后不会丢失。
                <br />
                如需清除某个平台的登录状态，在页面顶部的导航栏点击 🗑️ 按钮。
              </p>
            </div>
          </section>
        </div>

        <div className="modal-footer settings-footer">
          <button
            className="btn btn-danger"
            onClick={() => {
              if (confirm('确定恢复默认设置？这将清空所有自定义平台和配置。')) {
                resetToDefaults()
              }
            }}
          >
            恢复默认
          </button>
          <button className="btn btn-primary" onClick={handleClose}>
            完成
          </button>
        </div>
      </div>
    </div>
  )
}

export default SettingsModal
