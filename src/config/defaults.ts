import type { Platform, AppConfig } from '../types'

// 默认 AI 平台列表
export const DEFAULT_PLATFORMS: Platform[] = [
  {
    id: 'doubao',
    name: '豆包',
    url: 'https://www.doubao.com/chat/',
    icon: 'doubao.com',
    iconType: 'favicon',
    enabled: true,
    order: 0,
    description: '字节跳动 AI 对话助手',
  },
  {
    id: 'qwen',
    name: '千问',
    url: 'https://www.qianwen.com/',
    icon: 'chat.qwen.ai',
    iconType: 'favicon',
    enabled: true,
    order: 1,
    description: '阿里云通义千问 AI 助手',
  },
  {
    id: 'deepseek',
    name: 'DeepSeek',
    url: 'https://chat.deepseek.com/',
    icon: 'chat.deepseek.com',
    iconType: 'favicon',
    enabled: true,
    order: 2,
    description: '深度求索 AI 对话',
  },
  {
    id: 'chatgpt',
    name: 'ChatGPT',
    url: 'https://chatgpt.com/',
    icon: 'chatgpt.com',
    iconType: 'favicon',
    enabled: true,
    order: 3,
    description: 'OpenAI ChatGPT',
  },
  {
    id: 'claude',
    name: 'Claude',
    url: 'https://claude.ai/',
    icon: 'claude.ai',
    iconType: 'favicon',
    enabled: true,
    order: 4,
    description: 'Anthropic Claude AI 助手',
  },
]

// 默认应用配置
export const DEFAULT_CONFIG: AppConfig = {
  defaultPlatformId: 'doubao',
  rememberLastPlatform: true,
  lastPlatformId: undefined,
  theme: 'system',
  minimizeToTray: false,
  enableSplitView: false,
}
