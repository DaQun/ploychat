# PolyChat

English | [简体中文](README.zh-CN.md)

PolyChat is a Tauri v2, React, and TypeScript desktop client for multiple AI web apps. It brings commonly used AI chat websites into one lightweight desktop app, with each platform running in its own system WebView and local data directory so sessions can stay signed in while you switch quickly from the sidebar.

![PolyChat screenshot](docs/images/polychat-doubao.jpg)

The screenshot shows PolyChat's main workspace: the left sidebar is PolyChat's own platform switcher with search, shortcut hints, loading indicators, and login clones; the right side is the active platform rendered by a native WebView. The example opens Doubao while keeping its original web experience, and the sidebar can switch to Qwen, DeepSeek, ChatGPT, or Claude.

## Default Platforms

PolyChat currently ships with 5 default platforms:

- Doubao
- Qwen
- DeepSeek
- ChatGPT
- Claude

Users can still add, edit, duplicate, disable, remove, or reorder custom platforms in Settings.

## Features

- **System WebView**: built on Tauri v2 with WKWebView / WebView2 / WebKitGTK. Chromium is not bundled.
- **Persistent platform views**: enabled platforms keep their WebView instances alive, reducing reloads when switching.
- **Persistent login sessions**: platform data is stored under `app_data_dir/platforms/{platform_id}` so each platform keeps its own session by default.
- **In-platform tabs**: cross-origin popups and external links become in-app tabs, with controls to close the current tab or all secondary tabs.
- **Keyboard shortcuts**: `Ctrl/Cmd + 1~9` switches platforms, `Ctrl/Cmd + Tab` switches tabs inside the active platform, `Ctrl/Cmd + Shift + [ / ]` switches between recent conversations on the active platform, and `Ctrl/Cmd + Q` quits.
- **Download handling**: native downloads plus page-triggered `a[download]` / Blob downloads are saved to the system Downloads folder with success or failure toasts.
- **Data cleanup**: clear the active platform's login state and browsing data from the platform toolbar.
- **Open externally**: open the current page in the system default browser.
- **Platform management**: enable, disable, edit URLs, duplicate login clones, reorder by drag-and-drop, and restore defaults from Settings.

## Quick Start

### Requirements

- Node.js and npm
- Rust toolchain
- Tauri v2 system dependencies

Linux development and packaging also require WebKitGTK-related packages, commonly including `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, and `librsvg2-dev`.

### Development

```bash
npm install
npm run dev
```

`npm run dev` starts the Vite dev server and opens the Tauri desktop window.

Start only the frontend dev server:

```bash
npm run dev:vite
```

### Build

```bash
# Type-check and build the frontend
npm run build:vite

# Build the current platform app bundle
npm run build

# macOS DMG
npm run tauri:build:dmg
```

The configured Tauri bundle targets are `dmg`, `nsis`, `appimage`, and `deb`.

## Usage

### Switch Platforms

The left sidebar lists all enabled platforms. Click a platform to switch to it, or use `Ctrl/Cmd + 1~9` to switch by order.

### Manage Platforms

Open Settings from the bottom of the sidebar. You can:

- Add a platform
- Enable or disable a platform
- Edit name, URL, description, and User-Agent
- Duplicate a platform as an independent login clone
- Remove a platform
- Reorder platforms by drag-and-drop
- Restore default platforms and settings

### Login and Data Cleanup

Open a platform and sign in normally in the embedded website. Login state is saved locally in that platform's data directory.

To switch accounts or clear a session, click the cleanup button in the platform toolbar. PolyChat clears the platform's `localStorage`, `sessionStorage`, and WebView browsing data.

### Downloads

Downloads are saved to the system Downloads folder, and a toast appears in the sidebar area. Successful downloads include an "Open Folder" action.

File names prefer the WebView/server suggested download name. If a site only provides a generic download URL, PolyChat tries to extract a name from URL parameters such as `filename`, `name`, `title`, or `Content-Disposition`-style values.

## Architecture

### Frontend

- `src/App.tsx`: root component for the sidebar, WebView containers, settings modal, and download toasts.
- `src/components/Sidebar/`: platform list, collapsed state, and loading indicators.
- `src/components/WebViewContainer/`: creates native WebViews, syncs bounds, and manages in-platform tabs and navigation.
- `src/components/SettingsModal/`: platform and general app settings.
- `src/components/AddPlatformModal/`: custom platform creation.
- `src/store/platformStore.ts`: Zustand state, persisted to `localStorage`.
- `src/runtime/desktop.ts`: wrapper for Tauri commands and events.

### Rust / Tauri

- `src-tauri/src/lib.rs`: native WebView lifecycle, platform data directories, downloads, tab interception, shortcut bridging, and browsing-data cleanup.
- `src-tauri/tauri.conf.json`: window, build, and bundle configuration.
- `src-tauri/capabilities/default.json`: Tauri capability permissions.

The backend stores active WebViews in `Mutex<HashMap<String, PlatformView>>`. Each platform or in-platform tab maps to a native WebView. React owns layout, while Rust syncs native WebView position and size from frontend bounds.

## Tauri Commands

The frontend calls these commands through `src/runtime/desktop.ts`:

- `create_platform_view`
- `show_platform_view`
- `hide_all_platform_views`
- `close_platform_view`
- `set_platform_view_bounds`
- `navigate`
- `open_external`
- `clear_platform_data`
- `get_platform_state`
- `open_platform_tab`
- `dispatch_shortcut`
- `switch_conversation`
- `save_download_blob`
- `report_download_error`
- `quit_app`

Rust emits these main events to the frontend:

- `platform-state-changed`
- `platform-open-tab-requested`
- `platform-download-finished`
- `polychat-shortcut`

## Project Structure

```text
polychat/
├── src/
│   ├── App.tsx
│   ├── App.css
│   ├── main.tsx
│   ├── config/
│   │   └── defaults.ts
│   ├── runtime/
│   │   └── desktop.ts
│   ├── store/
│   │   └── platformStore.ts
│   ├── types/
│   │   └── index.ts
│   ├── utils/
│   │   └── platformIcons.ts
│   └── components/
│       ├── AddPlatformModal/
│       ├── SettingsModal/
│       ├── Sidebar/
│       └── WebViewContainer/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs
│   │   └── lib.rs
│   ├── capabilities/
│   │   └── default.json
│   ├── Cargo.toml
│   └── tauri.conf.json
├── public/
├── package.json
├── vite.config.ts
├── tsconfig.json
└── tsconfig.node.json
```

## Tech Stack

- Tauri v2
- React 18
- TypeScript 5
- Vite 6
- Zustand 4
- Rust 2021

## License

MIT
