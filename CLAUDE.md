# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# PolyChat — 项目说明

PolyChat:一站式多 AI 对话桌面客户端 (Tauri v2 + React + TypeScript)。默认集成豆包、千问、DeepSeek、ChatGPT、Claude 等 AI 平台的桌面应用，使用系统 WebView，产物体积约 4.6MB。

## 技术栈

| 层 | 技术 |
|---|---|
| 框架 | Tauri v2 + React 18 |
| 语言 | TypeScript 5 (strict) + Rust 2021 |
| 构建 | Vite 6 + @tauri-apps/cli |
| 状态管理 | Zustand 4 (持久化到 localStorage) |
| 打包 | tauri build (dmg/nsis/AppImage/deb) |

## 项目结构

```
polychat/
├── src/                     # React 前端
│   ├── App.tsx              # 根组件
│   ├── config/defaults.ts   # 默认平台列表 & 配置
│   ├── store/platformStore.ts  # Zustand 全局状态
│   ├── types/index.ts       # 类型定义
│   ├── runtime/desktop.ts   # Tauri IPC 封装层
│   ├── utils/platformIcons.ts  # Favicon 管理
│   └── components/
│       ├── Sidebar/
│       ├── WebViewContainer/
│       ├── BroadcastInput/   # 分屏广播输入框 + Prompt 模板库
│       ├── AddPlatformModal/
│       └── SettingsModal/
├── src-tauri/
│   ├── src/lib.rs           # Rust 后端：原生 WebView 管理
│   ├── Cargo.toml
│   └── tauri.conf.json      # 窗口配置、bundle 目标
└── dist/                    # 前端编译产物 (gitignored)
```

## 开发命令

```bash
npm run dev              # 启动开发模式 (Vite 1420端口 + Tauri)
npm run dev:vite         # 仅启动 Vite dev server
npm run build:vite       # tsc + vite build (前端)
npm run build            # tauri build --bundles app
npm run tauri:build:dmg  # macOS DMG 打包
```

## 架构要点

### Rust 后端 (`src-tauri/src/lib.rs`)

管理所有原生 WebView 的生命周期，核心数据结构是 `Mutex<HashMap<String, PlatformView>>`。

**Tauri 命令**（通过 `src/runtime/desktop.ts` 调用）：
- `create_platform_view` — 创建独立 WebView，数据目录为 `app_data_dir/platforms/{sanitized_id}`
- `show_platform_view` / `hide_all_platform_views` — 显示/隐藏控制
- `show_platform_views` — 分屏模式:同时显示多个 WebView(非互斥),其余隐藏
- `set_platform_view_bounds` — 同步 React 布局到原生 WebView 位置
- `navigate` — 通过 JS eval 执行 history.back/forward/reload
- `clear_platform_data` — 清除 localStorage、sessionStorage、数据目录
- `open_external` — 调用系统浏览器（macOS: `open`, Win: `cmd start`, Linux: `xdg-open`）
- `switch_conversation` — 在当前活跃平台 WebView 中执行启发式 DOM 脚本，定位历史会话列表并点击上/下一项（offset ±1）
- `fill_platform_input` — 广播填充:按 brand 分派各平台输入框选择器,文本经 `serde_json` 转义后注入(防 JS 注入),填充后延时模拟 Enter 自动发送

**菜单 & 快捷键**：`build_app_menu` 在系统默认菜单基础上 append `PolyChat` submenu，注册 `Previous/Next Conversation`（`CmdOrCtrl+Shift+[ / ]`）。菜单事件通过 `polychat-shortcut` 事件桥接到前端 `App.tsx` 的 `onShortcut`，再调用 `switch_conversation`。同样的快捷键判断也注入到每个 WebView 的 keydown 监听里，确保 WebView 焦点时也能响应。

**Tab 拦截**：创建 WebView 时注入 JS，覆盖 `window.open` 并拦截 ctrl/cmd-click，触发 `platform-open-tab-requested` 事件而非新窗口。

**状态同步**：注入 JS 每秒轮询 `document.title`，通过 `platform-state-changed` 事件推送给前端。

### 前端数据流 (`src/`)

1. **Zustand store** 持久化平台列表、配置、上次活跃平台 ID 到 localStorage
2. **App.tsx** 同时渲染所有启用平台的 `WebViewContainer`，用 CSS `display:none/block` 控制显隐（避免切换时重载页面）
3. **WebViewContainer** 监听 `isActive` prop → 调用 `desktop.ts` Tauri 命令 show/hide 原生 WebView，并用 `ResizeObserver` 同步 bounds
4. **Tauri 事件** (`platform-state-changed`, `platform-open-tab-requested`) → React 状态更新
5. **分屏与广播 + 模板库**：store 新增 `layoutMode` / `splitPlatformIds` / `promptTemplates`。分屏时 `App.tsx` 用 CSS grid 布局多个可见 `WebViewContainer` 并统一调 `show_platform_views`;`BroadcastInput` 提供广播输入框与 Prompt 模板库(支持 `{{变量}}` 占位符),经 `fill_platform_input` 注入各平台。整个功能由 `config.enableSplitView` 控制,默认关闭,在设置页开启。

### 平台图标 (`src/utils/platformIcons.ts`)

`KNOWN_FAVICON_URLS` 映射预置平台的 CDN 图标 URL。对未知平台，回退链：CDN → `/favicon.ico` → Google S2 favicon 服务。

### 类型定义 (`src/types/index.ts`)

- `Platform`: id / name / url / icon / iconType(`'emoji'|'url'|'favicon'`) / enabled / order / category / description / userAgent / injectScript
- `AppConfig`: defaultPlatformId / rememberLastPlatform / lastPlatformId / theme / minimizeToTray / enableNotifications / enableSplitView(分屏与广播开关,默认 false)
- `PromptTemplate`: id / title / content(可含 `{{变量}}` 占位符) / createdAt

## 常见任务

### 添加新 AI 平台

1. 在 `src/config/defaults.ts` 的 `DEFAULT_PLATFORMS` 中添加平台对象（id、name、url、icon、order、category）
2. 可选：在 `src/utils/platformIcons.ts` 的 `KNOWN_FAVICON_URLS` 中添加图标 URL
3. 可选：在 `src/config/defaults.ts` 的 `CATEGORY_COLORS`、`DEFAULT_CATEGORIES` 中添加分类

> 注意：Tauri v2 版本不需要维护域名映射。

### 调试

- **Rust 后端日志**：`npm run dev` 后在终端查看 `println!` / `eprintln!` 输出
- **前端日志**：Tauri 开发窗口的 DevTools Console
- **WebView 调试**：需在 `lib.rs` 中为特定平台的 WebView 启用开发者工具
- **清除平台数据**：设置弹窗中「清除登录状态」，或直接删除 `app_data_dir/platforms/{id}/` 目录

## 编码规范

- 文件名: kebab-case (`platformStore.ts`, `AddPlatformModal/`)
- 组件: PascalCase；变量/函数: camelCase
- CSS: 组件级 CSS 文件，每个组件独立目录
- 注释: 中英文均可，关键逻辑加中文注释
- 导入顺序: React → 第三方库 → 本地模块
