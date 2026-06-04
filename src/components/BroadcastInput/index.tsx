import React, { useState, useCallback, useMemo } from 'react'
import type { Platform, PromptTemplate } from '../../types'
import { fillPlatformInput } from '../../runtime/desktop'
import { usePlatformStore } from '../../store/platformStore'
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

// 提取模板中的 {{变量}} 名（去重保序）
function extractVariables(content: string): string[] {
  const re = /\{\{\s*([^}]+?)\s*\}\}/g
  const seen = new Set<string>()
  const out: string[] = []
  let m: RegExpExecArray | null
  while ((m = re.exec(content)) !== null) {
    const name = m[1]
    if (!seen.has(name)) {
      seen.add(name)
      out.push(name)
    }
  }
  return out
}

// 用变量值替换占位符；未填写的变量替换为空串
function renderTemplate(content: string, values: Record<string, string>): string {
  return content.replace(/\{\{\s*([^}]+?)\s*\}\}/g, (_, name: string) => values[name] ?? '')
}

const BroadcastInput: React.FC<BroadcastInputProps> = ({ visibleIds, platforms }) => {
  const { promptTemplates, addPromptTemplate, removePromptTemplate } = usePlatformStore()
  const [text, setText] = useState('')
  const [sending, setSending] = useState(false)

  // 模板弹层 / 变量填写 / 存为模板 的本地 UI 状态
  const [showTemplates, setShowTemplates] = useState(false)
  const [varTemplate, setVarTemplate] = useState<PromptTemplate | null>(null)
  const [varDraft, setVarDraft] = useState<Record<string, string>>({})
  const [showSave, setShowSave] = useState(false)
  const [saveTitle, setSaveTitle] = useState('')

  const targets = visibleIds
    .map(id => platforms.find(p => p.id === id))
    .filter((p): p is Platform => Boolean(p))

  const varNames = useMemo(
    () => (varTemplate ? extractVariables(varTemplate.content) : []),
    [varTemplate]
  )

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

  // 选用模板：无变量直接填入，有变量则打开变量填写面板
  const handlePickTemplate = useCallback((tpl: PromptTemplate) => {
    const vars = extractVariables(tpl.content)
    if (vars.length === 0) {
      setText(tpl.content)
      setShowTemplates(false)
      return
    }
    setVarTemplate(tpl)
    setVarDraft(Object.fromEntries(vars.map(v => [v, ''])))
    setShowTemplates(false)
  }, [])

  const handleConfirmVars = useCallback(() => {
    if (!varTemplate) return
    setText(renderTemplate(varTemplate.content, varDraft))
    setVarTemplate(null)
    setVarDraft({})
  }, [varTemplate, varDraft])

  const handleSaveTemplate = useCallback(() => {
    const content = text.trim()
    if (!content) return
    addPromptTemplate(saveTitle, content)
    setSaveTitle('')
    setShowSave(false)
  }, [text, saveTitle, addPromptTemplate])

  return (
    <div className="broadcast-input">
      <div className="broadcast-overlay">
        {showTemplates && (
          <div className="broadcast-tpl-pop">
            {promptTemplates.length === 0 ? (
              <div className="broadcast-tpl-empty">暂无模板，点「存为模板」保存当前内容</div>
            ) : (
              promptTemplates.map(tpl => (
                <div key={tpl.id} className="broadcast-tpl-item">
                  <button
                    type="button"
                    className="broadcast-tpl-pick"
                    title={tpl.content}
                    onClick={() => handlePickTemplate(tpl)}
                  >
                    {tpl.title}
                  </button>
                  <button
                    type="button"
                    className="broadcast-tpl-del"
                    title="删除模板"
                    onClick={() => removePromptTemplate(tpl.id)}
                  >
                    ✕
                  </button>
                </div>
              ))
            )}
          </div>
        )}

        {showSave && (
          <div className="broadcast-tpl-pop broadcast-save-pop">
            <input
              className="broadcast-save-input"
              value={saveTitle}
              placeholder="模板标题"
              autoFocus
              onChange={e => setSaveTitle(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter') handleSaveTemplate() }}
            />
            <button
              type="button"
              className="broadcast-tool-btn broadcast-save-confirm"
              disabled={!text.trim()}
              onClick={handleSaveTemplate}
            >
              保存
            </button>
          </div>
        )}

        {varTemplate && (
          <div className="broadcast-var-panel">
            <div className="broadcast-var-title">填写「{varTemplate.title}」的变量</div>
            {varNames.map(name => (
              <div key={name} className="broadcast-var-row">
                <label className="broadcast-var-label">{name}</label>
                <input
                  className="broadcast-var-input"
                  value={varDraft[name] ?? ''}
                  onChange={e => setVarDraft(d => ({ ...d, [name]: e.target.value }))}
                />
              </div>
            ))}
            <div className="broadcast-var-actions">
              <button
                type="button"
                className="broadcast-tool-btn"
                onClick={() => { setVarTemplate(null); setVarDraft({}) }}
              >
                取消
              </button>
              <button
                type="button"
                className="broadcast-btn broadcast-var-confirm"
                onClick={handleConfirmVars}
              >
                填入
              </button>
            </div>
          </div>
        )}
      </div>

      <div className="broadcast-toolbar">
        <button
          type="button"
          className="broadcast-tool-btn"
          onClick={() => { setShowTemplates(v => !v); setShowSave(false) }}
        >
          模板 ▾
        </button>
        <button
          type="button"
          className="broadcast-tool-btn"
          disabled={!text.trim()}
          onClick={() => { setShowSave(v => !v); setShowTemplates(false); setSaveTitle('') }}
        >
          存为模板
        </button>

        <div className="broadcast-targets" title="广播目标（分屏中可见的平台）">
          {targets.length > 0 ? (
            targets.map(p => (
              <span key={p.id} className="broadcast-chip">{p.name}</span>
            ))
          ) : (
            <span className="broadcast-chip broadcast-chip-empty">无可见平台</span>
          )}
        </div>
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
