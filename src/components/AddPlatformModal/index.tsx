import React, { useState } from 'react'
import type { Platform } from '../../types'
import { usePlatformStore } from '../../store/platformStore'
import { normalizeIconDomain } from '../../utils/platformIcons'
import './styles.css'

const AddPlatformModal: React.FC = () => {
  const { platforms, addPlatform, setShowAddModal } = usePlatformStore()

  const [name, setName] = useState('')
  const [url, setUrl] = useState('')
  const [description, setDescription] = useState('')
  const [error, setError] = useState('')

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()

    // 验证
    if (!name.trim()) {
      setError('请输入平台名称')
      return
    }
    if (!url.trim()) {
      setError('请输入平台 URL')
      return
    }

    // 验证 URL 格式
    const finalUrl = url.startsWith('http') ? url : `https://${url}`
    let parsedHost: string
    try {
      parsedHost = new URL(finalUrl).hostname
    } catch {
      setError('请输入有效的 URL')
      return
    }

    // 检查是否已存在
    const id = name.trim().toLowerCase().replace(/[^a-z0-9]/g, '-')
    if (platforms.find(p => p.id === id)) {
      setError('该平台名称已存在')
      return
    }

    // 自动从 URL 提取域名作为 favicon 来源，运行时按 CDN → /favicon.ico → Google S2 回退
    const iconDomain = normalizeIconDomain(parsedHost)

    const newPlatform: Platform = {
      id,
      name: name.trim(),
      url: finalUrl,
      icon: iconDomain,
      iconType: 'favicon',
      enabled: true,
      order: platforms.length,
      description: description.trim() || undefined,
    }

    addPlatform(newPlatform)
  }

  const handleClose = () => {
    setShowAddModal(false)
  }

  // 点击遮罩关闭
  const handleOverlayClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) {
      handleClose()
    }
  }

  return (
    <div className="modal-overlay" onClick={handleOverlayClick}>
      <div className="modal add-platform-modal">
        <div className="modal-header">
          <h2>添加 AI 平台</h2>
          <button className="btn-icon modal-close" onClick={handleClose}>✕</button>
        </div>

        <form onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <div className="modal-error">{error}</div>}

            <div className="form-group">
              <label className="form-label">平台名称 *</label>
              <input
                className="input"
                type="text"
                placeholder="例如：文心一言"
                value={name}
                onChange={e => { setName(e.target.value); setError('') }}
                autoFocus
              />
            </div>

            <div className="form-group">
              <label className="form-label">网页 URL *</label>
              <input
                className="input"
                type="text"
                placeholder="例如：https://yiyan.baidu.com/"
                value={url}
                onChange={e => { setUrl(e.target.value); setError('') }}
              />
              <p className="form-hint">图标将自动从网站 favicon 获取</p>
            </div>

            <div className="form-group">
              <label className="form-label">备注（可选）</label>
              <input
                className="input"
                type="text"
                placeholder="简短描述该平台"
                value={description}
                onChange={e => setDescription(e.target.value)}
              />
            </div>
          </div>

          <div className="modal-footer">
            <button type="button" className="btn" onClick={handleClose}>取消</button>
            <button type="submit" className="btn btn-primary">添加</button>
          </div>
        </form>
      </div>
    </div>
  )
}

export default AddPlatformModal
