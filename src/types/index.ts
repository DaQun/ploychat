// AI 平台定义
export interface Platform {
  /** 唯一标识 */
  id: string
  /** 显示名称 */
  name: string
  /** 网页 URL */
  url: string
  /** 图标值：emoji 字符、完整 URL，或用于解析 favicon 的域名 */
  icon: string
  /** 图标类型: 'emoji' | 'url' | 'favicon' */
  iconType: 'emoji' | 'url' | 'favicon'
  /** 是否启用 */
  enabled: boolean
  /** 排序权重 */
  order: number
  /** 备注 */
  description?: string
  /** 自定义 User-Agent */
  userAgent?: string
  /** 额外的注入脚本 */
  injectScript?: string
}

// 应用配置
export interface AppConfig {
  /** 窗口启动时默认打开的平台 ID */
  defaultPlatformId: string
  /** 是否记住上次打开的平台 */
  rememberLastPlatform: boolean
  /** 上次打开的平台 ID */
  lastPlatformId?: string
  /** 主题: light | dark | system */
  theme: 'light' | 'dark' | 'system'
  /** 启动时是否最小化到托盘 */
  minimizeToTray: boolean
  /** 是否启用分屏与广播功能（默认关闭） */
  enableSplitView?: boolean
}
