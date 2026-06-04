import React, { useState, useCallback } from 'react'
import type { Platform } from '../../types'
import { fillPlatformInput } from '../../runtime/desktop'
import './styles.css'

interface BroadcastInputProps {
  // 当前分屏中可见的平台 ID（广播仅发往这些平台）
  visibleIds: string[]
  platforms: Platform[]
}

// 从 view id 还原 brand：克隆分身 id 形如 `{base}__clone_{ts}`，取 base 段
function resolveBrand(id: string): string {
  return id.split('__clone_')[0]
}

const BroadcastInput: React.FC<BroadcastInputProps> = ({ visibleIds, platforms }) => {
  const [text, setText] = useState('')
  const [sending, setSending] = useState(false)

  const targets = visibleIds
    .map(id => platforms.find(p => p.id === id))
    .filter((p): p is Platform => Boolean(p))

  const handleBroadcast = useCallback(async () => {
    const value = text.trim()
    if (!value || visibleIds.length === 0 || sending) return
    setSending(true)
    try {
      await Promise.all(
        visibleIds.map(id =>
          fillPlatformInput(id, resolveBrand(id), value).catch(() => {})
        )
      )
      setText('')
    } finally {
      setSending(false)
    }
  }, [text, visibleIds, sending])

  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Cmd/Ctrl + Enter 触发广播填充
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault()
      handleBroadcast()
    }
  }, [handleBroadcast])

  return (
    <div className="broadcast-input">
      <div className="broadcast-targets" title="广播目标（分屏中可见的平台）">
        {targets.length > 0 ? (
          targets.map(p => (
            <span key={p.id} className="broadcast-chip">{p.name}</span>
          ))
        ) : (
          <span className="broadcast-chip broadcast-chip-empty">无可见平台</span>
        )}
      </div>
      <div className="broadcast-row">
        <textarea
          className="broadcast-textarea"
          value={text}
          placeholder="输入要广播到各平台的内容…（Cmd/Ctrl+Enter 发送，自动填充并提交）"
          onChange={e => setText(e.target.value)}
          onKeyDown={handleKeyDown}
          rows={2}
        />
        <button
          type="button"
          className="broadcast-btn"
          disabled={!text.trim() || visibleIds.length === 0 || sending}
          onClick={handleBroadcast}
        >
          {sending ? '发送中…' : '广播发送'}
        </button>
      </div>
    </div>
  )
}

export default BroadcastInput
