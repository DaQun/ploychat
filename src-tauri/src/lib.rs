use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, MenuItem, Submenu};
use tauri::webview::{DownloadEvent, NewWindowResponse};
use tauri::{
  AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Rect, Runtime, Webview,
  WebviewBuilder, WebviewUrl, Window,
};
use url::Url;

// macOS 下 Tauri 使用系统 WKWebView（与 Safari 同引擎），UA 必须与实际 JS/HTTP 指纹一致，
// 否则 Cloudflare、reCAPTCHA 等会因 UA 声称 Chrome 但缺少 Sec-CH-UA / userAgentData 等特征
// 判定为机器人。Windows 用 WebView2（Chromium），Linux 用 WebKitGTK，分别选用匹配的 UA。
#[cfg(target_os = "macos")]
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.2 Safari/605.1.15";

#[cfg(target_os = "windows")]
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0";

#[cfg(all(unix, not(target_os = "macos")))]
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

const MENU_ID_PREV_CONVERSATION: &str = "polychat-prev-conversation";
const MENU_ID_NEXT_CONVERSATION: &str = "polychat-next-conversation";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewBounds {
  x: f64,
  y: f64,
  width: f64,
  height: f64,
  // React 视口逻辑高度(window.innerHeight)，用于推算标题栏偏移
  viewport_height: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformState {
  platform_id: String,
  title: String,
  can_go_back: bool,
  can_go_forward: bool,
  loading: bool,
  url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenTabRequest {
  platform_id: String,
  opener_view_id: String,
  url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutEvent {
  action: String,
  index: Option<u32>,
  offset: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadFinishedEvent {
  platform_id: String,
  url: String,
  filename: Option<String>,
  path: Option<String>,
  success: bool,
}

#[derive(Debug)]
struct PlatformViewState {
  title: String,
  can_go_back: bool,
  can_go_forward: bool,
  loading: bool,
  url: String,
}

struct PlatformView {
  webview: Webview,
  state: PlatformViewState,
}

#[derive(Default)]
struct PlatformViews(Mutex<HashMap<String, PlatformView>>);

fn sanitize_platform_id(platform_id: &str) -> String {
  platform_id
    .chars()
    .map(|ch| {
      if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
        ch
      } else {
        '_'
      }
    })
    .collect()
}

fn data_store_identifier(storage_id: &str) -> [u8; 16] {
  let mut hash = 0xcbf29ce484222325u64;
  for byte in storage_id.as_bytes() {
    hash ^= *byte as u64;
    hash = hash.wrapping_mul(0x100000001b3);
  }

  let mut reverse_hash = 0x84222325cbf29ce4u64;
  for byte in storage_id.as_bytes().iter().rev() {
    reverse_hash ^= *byte as u64;
    reverse_hash = reverse_hash.wrapping_mul(0x100000001b3);
  }

  let mut identifier = [0u8; 16];
  identifier[..8].copy_from_slice(&hash.to_be_bytes());
  identifier[8..].copy_from_slice(&reverse_hash.to_be_bytes());
  identifier
}

fn physical_position_from_bounds(window: &Window, bounds: &ViewBounds) -> PhysicalPosition<i32> {
  let scale_factor = window.scale_factor().unwrap_or(1.0);

  // 子 WebView 的坐标原点是窗口内容区顶部（含标题栏下沿），
  // 而 React 的 getBoundingClientRect 原点是 React 视口顶部（标题栏之下）。
  // 二者相差一个标题栏高度，需要补偿到 y 上，否则子 WebView 整体上移、底部留白。
  let offset_y = match (bounds.viewport_height, window.inner_size().ok()) {
    (Some(viewport_height), Some(inner)) => {
      (inner.height as f64 - viewport_height * scale_factor).max(0.0)
    }
    _ => 0.0,
  };

  PhysicalPosition {
    x: (bounds.x * scale_factor).round() as i32,
    y: (bounds.y * scale_factor + offset_y).round() as i32,
  }
}

fn physical_size_from_bounds(window: &Window, bounds: &ViewBounds) -> PhysicalSize<u32> {
  let scale_factor = window.scale_factor().unwrap_or(1.0);

  PhysicalSize {
    width: (bounds.width * scale_factor).round().max(1.0) as u32,
    height: (bounds.height * scale_factor).round().max(1.0) as u32,
  }
}

fn rect_from_bounds(window: &Window, bounds: ViewBounds) -> Rect {
  Rect {
    position: tauri::Position::Physical(physical_position_from_bounds(window, &bounds)),
    size: tauri::Size::Physical(physical_size_from_bounds(window, &bounds)),
  }
}

fn emit_platform_state<R: Runtime>(
  app: &AppHandle<R>,
  platform_id: &str,
  state: &PlatformViewState,
) {
  let _ = app.emit(
    "platform-state-changed",
    PlatformState {
      platform_id: platform_id.to_string(),
      title: state.title.clone(),
      can_go_back: state.can_go_back,
      can_go_forward: state.can_go_forward,
      loading: state.loading,
      url: state.url.clone(),
    },
  );
}

fn emit_open_tab_request<R: Runtime>(
  app: &AppHandle<R>,
  platform_id: &str,
  opener_view_id: &str,
  url: &str,
) {
  let _ = app.emit(
    "platform-open-tab-requested",
    OpenTabRequest {
      platform_id: platform_id.to_string(),
      opener_view_id: opener_view_id.to_string(),
      url: url.to_string(),
    },
  );
}

fn is_external_http_navigation(url: &Url, current_url: &str) -> bool {
  if !matches!(url.scheme(), "http" | "https") {
    return false;
  }

  let Ok(current_url) = Url::parse(current_url) else {
    return false;
  };

  if !matches!(current_url.scheme(), "http" | "https") {
    return false;
  }

  url.origin().ascii_serialization() != current_url.origin().ascii_serialization()
}

fn is_auth_popup_url(url: &Url) -> bool {
  matches!(
    url.host_str(),
    Some("accounts.google.com")
      | Some("myaccount.google.com")
      | Some("oauth2.googleapis.com")
      | Some("anthropic.com")
      | Some("claude.ai")
  )
}

fn open_url_with_system_browser(url: &str) -> Result<(), String> {
  #[cfg(target_os = "macos")]
  let mut command = {
    let mut command = Command::new("open");
    command.arg(url);
    command
  };

  #[cfg(target_os = "windows")]
  let mut command = {
    let mut command = Command::new("cmd");
    command.args(["/C", "start", "", url]);
    command
  };

  #[cfg(all(unix, not(target_os = "macos")))]
  let mut command = {
    let mut command = Command::new("xdg-open");
    command.arg(url);
    command
  };

  command.spawn().map_err(|err| err.to_string())?;
  Ok(())
}

fn guess_download_filename(url: &Url) -> String {
  if let Some(filename) = filename_from_query(url) {
    return sanitize_filename(&filename);
  }

  let raw = url
    .path_segments()
    .and_then(|mut s| s.next_back())
    .and_then(|seg| percent_decode_segment(seg))
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "download".to_string());
  // 去掉文件系统不友好字符
  raw
    .chars()
    .map(|c| match c {
      '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
      _ => c,
    })
    .collect()
}

fn preferred_download_filename(url: &Url, destination: &PathBuf) -> String {
  let existing = destination
    .file_name()
    .and_then(|name| name.to_str())
    .map(|name| name.trim())
    .filter(|name| !name.is_empty())
    .filter(|name| !is_generic_download_name(name));

  existing
    .map(sanitize_filename)
    .unwrap_or_else(|| guess_download_filename(url))
}

fn is_generic_download_name(name: &str) -> bool {
  let lower = name.trim().to_ascii_lowercase();
  let stem = lower.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(&lower);
  stem == "download" || stem == "file" || stem == "untitled"
}

fn filename_from_query(url: &Url) -> Option<String> {
  for (key, value) in url.query_pairs() {
    let key = key.to_ascii_lowercase();
    if matches!(
      key.as_str(),
      "response-content-disposition" | "content-disposition" | "content_disposition"
    ) {
      if let Some(filename) = filename_from_content_disposition(&value) {
        return Some(filename);
      }
    }
  }

  const FILENAME_KEYS: &[&str] = &[
    "filename",
    "file_name",
    "file-name",
    "download_filename",
    "download-filename",
    "download_name",
    "download-name",
    "name",
    "title",
  ];

  for (key, value) in url.query_pairs() {
    let key = key.to_ascii_lowercase();
    if FILENAME_KEYS.contains(&key.as_str()) {
      let filename = value.trim();
      if is_plausible_filename(filename) {
        return Some(filename.to_string());
      }
    }
  }

  None
}

fn filename_from_content_disposition(value: &str) -> Option<String> {
  for part in value.split(';') {
    let part = part.trim();
    let lower = part.to_ascii_lowercase();
    if lower.starts_with("filename*=") {
      let raw = part.split_once('=')?.1.trim().trim_matches('"');
      let encoded = raw.rsplit_once("''").map(|(_, name)| name).unwrap_or(raw);
      let decoded = percent_decode_segment(encoded).unwrap_or_else(|| encoded.to_string());
      if is_plausible_filename(&decoded) {
        return Some(decoded);
      }
    }
  }

  for part in value.split(';') {
    let part = part.trim();
    let lower = part.to_ascii_lowercase();
    if lower.starts_with("filename=") {
      let raw = part.split_once('=')?.1.trim().trim_matches('"');
      if is_plausible_filename(raw) {
        return Some(raw.to_string());
      }
    }
  }

  None
}

fn is_plausible_filename(name: &str) -> bool {
  let trimmed = name.trim();
  !trimmed.is_empty()
    && trimmed.len() <= 180
    && !trimmed.contains('/')
    && !trimmed.contains('\\')
    && !trimmed.starts_with("http:")
    && !trimmed.starts_with("https:")
}

fn percent_decode_segment(segment: &str) -> Option<String> {
  // 简化的 percent-decode；失败就直接返回原串
  let bytes = segment.as_bytes();
  let mut out = Vec::with_capacity(bytes.len());
  let mut i = 0;
  while i < bytes.len() {
    if bytes[i] == b'%' && i + 2 < bytes.len() {
      let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
      if let Ok(v) = u8::from_str_radix(hex, 16) {
        out.push(v);
        i += 3;
        continue;
      }
    }
    out.push(bytes[i]);
    i += 1;
  }
  String::from_utf8(out).ok()
}

fn unique_path(candidate: PathBuf) -> PathBuf {
  if !candidate.exists() {
    return candidate;
  }
  let parent = candidate.parent().map(|p| p.to_path_buf()).unwrap_or_default();
  let stem = candidate
    .file_stem()
    .and_then(|s| s.to_str())
    .unwrap_or("download")
    .to_string();
  let ext = candidate
    .extension()
    .and_then(|s| s.to_str())
    .map(|s| format!(".{s}"))
    .unwrap_or_default();
  for i in 1..1000 {
    let next = parent.join(format!("{stem}-{i}{ext}"));
    if !next.exists() {
      return next;
    }
  }
  candidate
}

// 在每个新 document 创建后、页面 JS 执行之前最早注入。
// 用于把 WebView 在 navigator 上残留的"自动化指纹"修圆，避免 Cloudflare/Turnstile 等
// 反爬服务在 challenge 阶段直接把我们判定为机器人。
fn stealth_init_script() -> &'static str {
  r#"
(() => {
  if (window.__POLYCHAT_STEALTH__) return;
  window.__POLYCHAT_STEALTH__ = true;
  const safeDefine = (obj, prop, getter) => {
    try {
      Object.defineProperty(obj, prop, { get: getter, configurable: true });
    } catch (_) {}
  };
  // navigator.webdriver: 真实 Safari 没有此属性 (undefined)，部分自动化 WebView 会暴露为 true
  try { delete Navigator.prototype.webdriver; } catch (_) {}
  safeDefine(navigator, 'webdriver', () => undefined);

  // navigator.languages: 真实 Safari 通常返回 ['zh-CN','zh','en'] 之类的非空数组
  try {
    if (!navigator.languages || navigator.languages.length === 0) {
      const lang = navigator.language || 'en-US';
      const langs = lang.startsWith('zh') ? ['zh-CN', 'zh', 'en'] : [lang, 'en'];
      safeDefine(navigator, 'languages', () => langs);
    }
  } catch (_) {}

  // 部分检测脚本会读 window.chrome：UA 既然声明为 Safari，就应该没有 chrome 对象
  try {
    if (/Safari/.test(navigator.userAgent) && !/Chrome|Chromium|Edg/.test(navigator.userAgent)) {
      try { delete window.chrome; } catch (_) {}
    }
  } catch (_) {}

  // Notification.permission 在 WebView 中可能返回 'denied'，真实 Safari 默认 'default'
  try {
    if (typeof Notification !== 'undefined') {
      const original = Notification.permission;
      if (original === 'denied') {
        safeDefine(Notification, 'permission', () => 'default');
      }
    }
  } catch (_) {}

  // 拦 URL.createObjectURL：缓存 Blob 以便后面 a[download] 拦截不必再 fetch
  // 必须放在 init script 而非 page-load 注入，否则页面早期创建的 blob 拿不到。
  try {
    const originalCreateURL = URL.createObjectURL;
    if (originalCreateURL && !URL.__POLYCHAT_PATCHED__) {
      URL.__POLYCHAT_PATCHED__ = true;
      const cache = new Map();
      window.__POLYCHAT_BLOBS__ = cache;
      URL.createObjectURL = function(obj) {
        const url = originalCreateURL.call(URL, obj);
        try { if (obj instanceof Blob) cache.set(url, obj); } catch (_) {}
        return url;
      };
      const originalRevoke = URL.revokeObjectURL;
      if (originalRevoke) {
        URL.revokeObjectURL = function(url) {
          try { cache.delete(url); } catch (_) {}
          return originalRevoke.call(URL, url);
        };
      }
    }
  } catch (_) {}
})();
"#
}

fn tab_interceptor_script(platform_id: &str, opener_view_id: &str) -> String {
  let platform_id = serde_json::to_string(platform_id).unwrap_or_else(|_| "\"\"".to_string());
  let opener_view_id = serde_json::to_string(opener_view_id).unwrap_or_else(|_| "\"\"".to_string());

  format!(
    r#"
(() => {{
  if (window.__POLYCHAT_TAB_INTERCEPTOR__) return;
  window.__POLYCHAT_TAB_INTERCEPTOR__ = true;
  const platformId = {platform_id};
  const openerViewId = {opener_view_id};
  const polyDebugLog = (tag, payload) => {{
    try {{
      const text = typeof payload === 'string' ? payload : JSON.stringify(payload);
      window.__TAURI_INTERNALS__?.invoke('debug_log', {{ tag: String(tag), payload: text }});
    }} catch (_) {{}}
  }};
  // 修复 macOS WKWebView 拒绝 navigator.clipboard.write 写图片的限制。
  // WKWebView 不允许 ClipboardItem(image/*)，拦截 clipboard.write，把图片二进制
  // 通过 Tauri 命令交 arboard 写入系统剪贴板。
  // Windows/Linux 上的 WebView2/WebKitGTK 原生支持图片剪贴板，无需注入 hook。
  try {{
    if (/Mac/.test(navigator.userAgent)) {{
      const cb = navigator.clipboard;
      if (cb && cb.write && !cb.__polyImageHook) {{
        const orig = cb.write.bind(cb);
        const blobToBase64 = (blob) => new Promise((resolve, reject) => {{
          const reader = new FileReader();
          reader.onerror = () => reject(reader.error || new Error('FileReader error'));
          reader.onload = () => {{
            const result = String(reader.result || '');
            const idx = result.indexOf(',');
            resolve(idx >= 0 ? result.slice(idx + 1) : result);
          }};
          reader.readAsDataURL(blob);
        }});
        cb.write = async function(items) {{
          const list = Array.from(items || []);
          let imageBlob = null;
          try {{
            for (const item of list) {{
              const types = item.types || [];
              const imageType = types.find(t => /^image\//.test(t));
              if (imageType && typeof item.getType === 'function') {{
                imageBlob = await item.getType(imageType);
                break;
              }}
            }}
          }} catch (_) {{}}
          if (imageBlob) {{
            try {{
              const b64 = await blobToBase64(imageBlob);
              await window.__TAURI_INTERNALS__?.invoke('copy_image_to_clipboard', {{ dataBase64: b64 }});
              return undefined;
            }} catch (e) {{
              polyDebugLog('clipboard.image.native.fail', String(e && e.message || e));
            }}
          }}
          return orig(items);
        }};
        cb.__polyImageHook = true;
      }}
    }}
  }} catch (e) {{
    polyDebugLog('clipboard.image.hook.install.fail', String(e && e.message || e));
  }}
  const quitApp = () => {{
    try {{
      window.__TAURI_INTERNALS__?.invoke('quit_app');
    }} catch (_) {{}}
  }};
  const openInAppTab = (url) => {{
    if (!url) return;
    try {{
      const absoluteUrl = new URL(url, location.href).href;
      window.__TAURI_INTERNALS__?.invoke('open_platform_tab', {{
        platformId,
        openerViewId,
        url: absoluteUrl
      }});
    }} catch (_) {{}}
  }};
  const originalOpen = window.open;
  const isExternalUrl = (raw) => {{
    if (!raw) return false;
    try {{
      const parsed = new URL(raw, location.href);
      if (!/^https?:$/.test(parsed.protocol)) return false;
      return parsed.origin !== location.origin;
    }} catch (_) {{
      return false;
    }}
  }};
  const isAuthPopupUrl = (raw) => {{
    if (!raw) return false;
    try {{
      const parsed = new URL(raw, location.href);
      return [
        'accounts.google.com',
        'myaccount.google.com',
        'oauth2.googleapis.com',
        'anthropic.com',
        'claude.ai'
      ].includes(parsed.hostname);
    }} catch (_) {{
      return false;
    }}
  }};
  window.open = function(url, target, features) {{
    // 仅拦截"真正跨域"的普通窗口打开请求；认证弹窗保留原生 popup，
    // 避免破坏 Google OAuth 这类依赖 window.opener / postMessage 的授权流程。
    if (url && isExternalUrl(url) && !isAuthPopupUrl(url)) {{
      openInAppTab(url);
      return null;
    }}
    return originalOpen ? originalOpen.call(window, url, target, features) : null;
  }};

  // 把 Blob/ArrayBuffer 转 base64，分块以避免大文件超过调用栈限制
  const toBase64 = (blob) => new Promise((resolve, reject) => {{
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error);
    reader.onload = () => {{
      const result = String(reader.result || '');
      const idx = result.indexOf(',');
      resolve(idx >= 0 ? result.slice(idx + 1) : result);
    }};
    reader.readAsDataURL(blob);
  }});

  const sendDownload = (blob, filename, sourceUrl) => {{
    toBase64(blob).then((b64) => {{
      try {{
        window.__TAURI_INTERNALS__?.invoke('save_download_blob', {{
          platformId,
          filename: filename || 'download',
          sourceUrl: sourceUrl || '',
          dataBase64: b64,
        }});
      }} catch (_) {{}}
    }}).catch((e) => {{
      console.warn('[polychat] toBase64 failed', e);
      reportDownloadFailure(filename, sourceUrl, (e && e.message) || 'toBase64 failed');
    }});
  }};

  // JS 拦截失败时上报，给用户一个可见的失败 toast
  const reportDownloadFailure = (filename, sourceUrl, reason) => {{
    console.warn('[polychat] download failed', {{ filename, sourceUrl, reason }});
    try {{
      window.__TAURI_INTERNALS__?.invoke('report_download_error', {{
        platformId,
        filename: filename || 'download',
        sourceUrl: sourceUrl || '',
        reason: String(reason || 'unknown'),
      }});
    }} catch (_) {{}}
  }};

  const openInSystemBrowser = (url) => {{
    try {{
      window.__TAURI_INTERNALS__?.invoke('open_external', {{ url }});
    }} catch (_) {{}}
  }};

  // 拦截 a[download]：a.click() / 用户点击带 download 属性的链接。
  // WKWebView 默认不会处理这类下载，必须自己读出内容写盘。
  const tryInterceptDownloadAnchor = (anchor, event) => {{
    if (!anchor || !anchor.hasAttribute('download')) return false;
    const href = anchor.getAttribute('href') || anchor.href || '';
    if (!href) return false;
    const filename = anchor.getAttribute('download') || href.split(/[\\/?#]/).filter(Boolean).pop() || 'download';
    console.log('[polychat] intercept a[download] click', {{ href, filename }});

    // 优先使用 createObjectURL 缓存里的 blob，避免再 fetch（很多站点的 CSP 不允许 connect-src blob:）
    const cachedBlob = window.__POLYCHAT_BLOBS__?.get(href);
    if (cachedBlob) {{
      if (event) {{
        event.preventDefault();
        event.stopPropagation();
      }}
      sendDownload(cachedBlob, filename, href);
      return true;
    }}

    // blob: 与 data: 直接读
    if (href.startsWith('blob:') || href.startsWith('data:')) {{
      if (event) {{
        event.preventDefault();
        event.stopPropagation();
      }}
      fetch(href).then(r => r.blob()).then(b => sendDownload(b, filename, href)).catch((e) => {{
        console.warn('[polychat] blob/data fetch failed', e);
        reportDownloadFailure(filename, href, (e && e.message) || 'blob/data fetch failed');
      }});
      return true;
    }}
    // http(s) 资源也接管下来，否则 WKWebView 多半直接在 webview 里跳转。
    // 同源带 credentials（保留登录态）；跨域强制不带 credentials，否则会触发 CORS 失败。
    try {{
      const url = new URL(href, location.href);
      if (/^https?:$/.test(url.protocol)) {{
        if (event) {{
          event.preventDefault();
          event.stopPropagation();
        }}
        const sameOrigin = url.origin === location.origin;
        const fetchInit = sameOrigin
          ? {{ credentials: 'include' }}
          : {{ credentials: 'omit', mode: 'cors' }};
        fetch(url.href, fetchInit)
          .then((r) => {{
            if (!r.ok) throw new Error('HTTP ' + r.status);
            return r.blob();
          }})
          .then((b) => sendDownload(b, filename, url.href))
          .catch((e) => {{
            console.warn('[polychat] http fetch failed', e);
            // 兜底：把链接抛给系统浏览器，让用户在 Safari 里完成下载，
            // 然后再上报一次失败让 toast 提示"已在浏览器中打开"。
            openInSystemBrowser(url.href);
            reportDownloadFailure(filename, url.href, '已在系统浏览器中打开 (' + ((e && e.message) || 'fetch failed') + ')');
          }});
        return true;
      }}
    }} catch (_) {{}}
    return false;
  }};

  // 兜底：当 DOM 里新增 <a download> 并被合成 click 时（很多框架的下载实现），
  // MutationObserver 看不到 click 行为，但可以观察"新增节点"。结合下面的 click 监听够覆盖大多数情况。
  // 但若网站走 window.location.href = blobUrl 这种方式，需要单独拦 location 赋值。
  try {{
    const proto = Object.getPrototypeOf(window.location);
    const desc = Object.getOwnPropertyDescriptor(Location.prototype, 'href') || Object.getOwnPropertyDescriptor(proto, 'href');
    if (desc && desc.set) {{
      const originalHrefSetter = desc.set.bind(window.location);
      Object.defineProperty(window.location, 'href', {{
        configurable: true,
        get: desc.get ? desc.get.bind(window.location) : () => '',
        set(value) {{
          const v = String(value || '');
          if (v.startsWith('blob:') || v.startsWith('data:')) {{
            console.log('[polychat] intercept location.href blob/data set', v);
            const filename = (v.split('#').pop() || 'download').replace(/[^A-Za-z0-9._\-]/g, '_');
            fetch(v).then(r => r.blob()).then(b => sendDownload(b, filename, v)).catch((e) => {{
              console.warn('[polychat] location blob fetch failed', e);
              reportDownloadFailure(filename, v, (e && e.message) || 'location blob fetch failed');
            }});
            return;
          }}
          originalHrefSetter(value);
        }}
      }});
    }}
  }} catch (e) {{
    console.warn('[polychat] hook location.href failed', e);
  }}

  // createObjectURL 拦截已挪到 stealth_init_script（启动更早），这里不再重复 hook。

  // 兜底快捷键：Cmd/Ctrl + Shift + S 保存当前鼠标悬停的图片到 Downloads。
  // 用于网站下载按钮不工作时的人工兜底。
  try {{
    let lastHoverImg = null;
    document.addEventListener('mouseover', (e) => {{
      const t = e.target;
      if (t && t.tagName === 'IMG') lastHoverImg = t;
    }}, true);
    document.addEventListener('keydown', (e) => {{
      if (!(e.metaKey || e.ctrlKey) || !e.shiftKey) return;
      if (String(e.key || '').toLowerCase() !== 's') return;
      const img = lastHoverImg;
      if (!img || !img.src) return;
      e.preventDefault();
      e.stopPropagation();
      const src = img.src;
      const filename = (img.alt || src.split(/[\\/?#]/).filter(Boolean).pop() || 'image').slice(0, 80);
      console.log('[polychat] hotkey save image', src);
      fetch(src, {{ credentials: 'include', mode: 'cors' }})
        .then(r => r.blob())
        .then(b => sendDownload(b, filename, src))
        .catch((err) => {{
          console.warn('[polychat] hotkey fetch failed, fallback to canvas', err);
          // CORS 失败时尝试从已加载的 <img> 画到 canvas 再读
          try {{
            const canvas = document.createElement('canvas');
            canvas.width = img.naturalWidth || img.width;
            canvas.height = img.naturalHeight || img.height;
            canvas.getContext('2d').drawImage(img, 0, 0);
            canvas.toBlob((b) => b && sendDownload(b, (filename || 'image') + '.png', src), 'image/png');
          }} catch (e2) {{
            console.warn('[polychat] canvas fallback failed', e2);
          }}
        }});
    }}, true);
  }} catch (_) {{}}

  const shouldOpenInAppTab = (anchor) => {{
    if (!anchor || !anchor.href) return false;
    if (anchor.hasAttribute('download')) return false;
    const rawHref = anchor.getAttribute('href') || '';
    if (!rawHref || rawHref.startsWith('#')) return false;

    let parsed;
    try {{
      parsed = new URL(anchor.href, location.href);
    }} catch (_) {{
      return false;
    }}

    if (!/^https?:$/.test(parsed.protocol)) return false;
    if (
      parsed.origin === location.origin &&
      parsed.pathname === location.pathname &&
      parsed.search === location.search &&
      parsed.hash
    ) {{
      return false;
    }}

    return parsed.href !== location.href;
  }};
  // 程式调用 a.click()（很多 SPA 用这种方式触发下载）不会冒泡到 document，
  // 直接在原型层面拦下来。
  try {{
    const originalClick = HTMLAnchorElement.prototype.click;
    HTMLAnchorElement.prototype.click = function() {{
      if (this.hasAttribute('download')) {{
        if (tryInterceptDownloadAnchor(this, null)) {{
          return;
        }}
      }}
      return originalClick.apply(this, arguments);
    }};
  }} catch (_) {{}}

  document.addEventListener('click', (event) => {{
    const path = event.composedPath ? event.composedPath() : [];
    let anchor = path.find((item) => item && item.tagName === 'A');
    if (!anchor && event.target?.closest) anchor = event.target.closest('a[href]');
    if (tryInterceptDownloadAnchor(anchor, event)) return;
    const target = (anchor?.target || '').toLowerCase();
    const isExternal = (() => {{
      try {{ return anchor?.href && new URL(anchor.href).origin !== location.origin; }} catch (_) {{ return false; }}
    }})();
    if (
      shouldOpenInAppTab(anchor) &&
      (target === '_blank' || isExternal || event.metaKey || event.ctrlKey || event.shiftKey)
    ) {{
      event.preventDefault();
      event.stopPropagation();
      openInAppTab(anchor.href);
    }}
  }}, true);
  document.addEventListener('auxclick', (event) => {{
    if (event.button !== 1) return;
    const path = event.composedPath ? event.composedPath() : [];
    let anchor = path.find((item) => item && item.tagName === 'A');
    if (!anchor && event.target?.closest) anchor = event.target.closest('a[href]');
    if (shouldOpenInAppTab(anchor)) {{
      event.preventDefault();
      event.stopPropagation();
      openInAppTab(anchor.href);
    }}
  }}, true);
  const dispatchShortcut = (action, extra) => {{
    try {{
      window.__TAURI_INTERNALS__?.invoke('dispatch_shortcut', Object.assign({{ action }}, extra || {{}}));
    }} catch (_) {{}}
  }};
  const ensureFindBox = () => {{
    let box = document.getElementById('__polychat_find_box__');
    if (box) return box;

    box = document.createElement('div');
    box.id = '__polychat_find_box__';
    box.style.cssText = [
      'position:fixed',
      'top:12px',
      'right:12px',
      'z-index:2147483647',
      'display:none',
      'align-items:center',
      'gap:6px',
      'height:36px',
      'padding:6px 8px',
      'border:1px solid rgba(0,0,0,.18)',
      'border-radius:8px',
      'background:rgba(255,255,255,.98)',
      'box-shadow:0 8px 24px rgba(0,0,0,.18)',
      'font:13px -apple-system,BlinkMacSystemFont,Segoe UI,sans-serif'
    ].join(';');

    const input = document.createElement('input');
    input.type = 'search';
    input.placeholder = 'Find';
    input.autocomplete = 'off';
    input.style.cssText = [
      'width:220px',
      'height:24px',
      'border:1px solid rgba(0,0,0,.22)',
      'border-radius:5px',
      'padding:0 7px',
      'font:13px -apple-system,BlinkMacSystemFont,Segoe UI,sans-serif',
      'outline:none',
      'color:#111',
      'background:#fff'
    ].join(';');

    const status = document.createElement('span');
    status.textContent = '0/0';
    status.style.cssText = [
      'min-width:34px',
      'color:#555',
      'font:12px -apple-system,BlinkMacSystemFont,Segoe UI,sans-serif',
      'text-align:center',
      'white-space:nowrap'
    ].join(';');

    const makeButton = (text, title) => {{
      const button = document.createElement('button');
      button.type = 'button';
      button.textContent = text;
      button.title = title;
      button.style.cssText = [
        'width:24px',
        'height:24px',
        'border:0',
        'border-radius:5px',
        'background:transparent',
        'color:#333',
        'font:15px/22px -apple-system,BlinkMacSystemFont,Segoe UI,sans-serif',
        'cursor:pointer'
      ].join(';');
      return button;
    }};
    const prev = makeButton('↑', 'Previous');
    const next = makeButton('↓', 'Next');

    const close = document.createElement('button');
    close.type = 'button';
    close.textContent = '×';
    close.title = 'Close';
    close.style.cssText = [
      'width:24px',
      'height:24px',
      'border:0',
      'border-radius:5px',
      'background:transparent',
      'color:#333',
      'font:18px/22px -apple-system,BlinkMacSystemFont,Segoe UI,sans-serif',
      'cursor:pointer'
    ].join(';');

    let searchTimer = null;
    let matches = [];
    let activeMatchIndex = -1;

    const restoreInputFocus = () => {{
      setTimeout(() => {{
        try {{
          input.focus();
          input.setSelectionRange(input.value.length, input.value.length);
        }} catch (_) {{}}
      }}, 0);
    }};

    const updateStatus = () => {{
      status.textContent = matches.length > 0 ? String(activeMatchIndex + 1) + '/' + String(matches.length) : '0/0';
    }};

    const unwrapHighlights = () => {{
      const highlighted = Array.from(document.querySelectorAll('mark.__polychat_find_match__'));
      for (const mark of highlighted) {{
        const parent = mark.parentNode;
        if (!parent) continue;
        parent.replaceChild(document.createTextNode(mark.textContent || ''), mark);
        parent.normalize();
      }}
      matches = [];
      activeMatchIndex = -1;
      updateStatus();
    }};

    const canSearchNode = (node) => {{
      const parent = node.parentElement;
      if (!parent) return false;
      if (parent.closest('#__polychat_find_box__')) return false;
      const tag = parent.tagName;
      if (['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEXTAREA', 'INPUT', 'SELECT', 'OPTION'].includes(tag)) return false;
      return Boolean(node.nodeValue && node.nodeValue.trim());
    }};

    const highlightNode = (node, queryLower) => {{
      const text = node.nodeValue || '';
      const textLower = text.toLowerCase();
      let offset = 0;
      let hit = textLower.indexOf(queryLower, offset);
      if (hit < 0) return;

      const fragment = document.createDocumentFragment();
      while (hit >= 0) {{
        if (hit > offset) {{
          fragment.appendChild(document.createTextNode(text.slice(offset, hit)));
        }}
        const mark = document.createElement('mark');
        mark.className = '__polychat_find_match__';
        mark.textContent = text.slice(hit, hit + queryLower.length);
        mark.style.cssText = 'background:#ffe66d;color:inherit;padding:0;border-radius:2px;';
        fragment.appendChild(mark);
        matches.push(mark);
        offset = hit + queryLower.length;
        hit = textLower.indexOf(queryLower, offset);
      }}
      if (offset < text.length) {{
        fragment.appendChild(document.createTextNode(text.slice(offset)));
      }}
      node.parentNode && node.parentNode.replaceChild(fragment, node);
    }};

    const collectMatches = (query) => {{
      unwrapHighlights();
      const normalized = String(query || '').trim().toLowerCase();
      if (!normalized) return;
      const walker = document.createTreeWalker(document.body || document.documentElement, NodeFilter.SHOW_TEXT, {{
        acceptNode(node) {{
          return canSearchNode(node) ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT;
        }}
      }});
      const nodes = [];
      while (walker.nextNode()) nodes.push(walker.currentNode);
      for (const node of nodes) highlightNode(node, normalized);
      updateStatus();
    }};

    const activateMatch = (index) => {{
      if (!matches.length) {{
        updateStatus();
        restoreInputFocus();
        return;
      }}
      if (activeMatchIndex >= 0 && matches[activeMatchIndex]) {{
        matches[activeMatchIndex].style.background = '#ffe66d';
        matches[activeMatchIndex].style.outline = 'none';
      }}
      activeMatchIndex = (index + matches.length) % matches.length;
      const match = matches[activeMatchIndex];
      match.style.background = '#ff9f1c';
      match.style.outline = '1px solid #d86b00';
      match.scrollIntoView({{ block: 'center', inline: 'nearest' }});
      updateStatus();
      restoreInputFocus();
    }};

    const runFind = (backward) => {{
      const query = input.value;
      collectMatches(query);
      if (!matches.length) {{
        restoreInputFocus();
        return;
      }}
      activateMatch(backward ? matches.length - 1 : 0);
    }};

    const scheduleFind = (backward) => {{
      if (searchTimer) clearTimeout(searchTimer);
      searchTimer = setTimeout(() => {{
        searchTimer = null;
        runFind(backward);
      }}, 180);
    }};

    close.addEventListener('click', () => {{
      if (searchTimer) {{
        clearTimeout(searchTimer);
        searchTimer = null;
      }}
      box.style.display = 'none';
      unwrapHighlights();
    }});
    prev.addEventListener('click', () => activateMatch(activeMatchIndex - 1));
    next.addEventListener('click', () => activateMatch(activeMatchIndex + 1));
    input.addEventListener('keydown', (event) => {{
      event.stopPropagation();
      if (event.key === 'Escape') {{
        event.preventDefault();
        box.style.display = 'none';
        unwrapHighlights();
        return;
      }}
      if (event.key === 'Enter') {{
        event.preventDefault();
        if (!matches.length) {{
          runFind(event.shiftKey);
        }} else {{
          activateMatch(activeMatchIndex + (event.shiftKey ? -1 : 1));
        }}
      }}
    }}, true);
    input.addEventListener('input', () => scheduleFind(false));

    box.appendChild(input);
    box.appendChild(status);
    box.appendChild(prev);
    box.appendChild(next);
    box.appendChild(close);
    (document.body || document.documentElement).appendChild(box);
    box.__polychatScheduleFind = scheduleFind;
    return box;
  }};
  const openFindBox = () => {{
    const box = ensureFindBox();
    const input = box.querySelector('input');
    const selectedText = String(window.getSelection?.() || '').trim();
    if (selectedText && selectedText.length <= 120) input.value = selectedText;
    box.style.display = 'flex';
    input.focus();
    input.select();
    if (input.value) {{
      box.__polychatScheduleFind && box.__polychatScheduleFind(false);
    }}
  }};
  const EXCLUDED_CONVERSATION_TEXT_RE = /^(new chat|new conversation|新建|新对话|新会话|新聊天|ai 创作|ai创作|云盘|更多|我的创作)$/i;
  const DATE_GROUP_TEXT_RE = /^\d{{4}}(?:[-/年]\s*)?(?:0?[1-9]|1[0-2])月?$/;
  const ACTIVE_CLASS_RE = /(?:^|[\s_-])(active|selected|current|is-active|is-selected)(?:$|[\s_-])/;
  const normalizeConversationText = (text) => String(text || '').replace(/[⋯…]/g, '').replace(/\s+/g, ' ').trim();
  const isSelectedByStyle = (el) => {{
    let cursor = el;
    for (let depth = 0; cursor && depth < 4; depth++, cursor = cursor.parentElement) {{
      const style = getComputedStyle(cursor);
      const color = style.backgroundColor || '';
      const match = color.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/i);
      if (!match) continue;
      const r = Number(match[1]);
      const g = Number(match[2]);
      const b = Number(match[3]);
      if (b > r + 15 && b >= g + 5) return true;
    }}
    return false;
  }};
  const getCurrentConversationTitles = () => {{
    const titles = [];
    const add = (value) => {{
      const text = normalizeConversationText(value);
      if (text.length >= 3 && text.length <= 120 && !EXCLUDED_CONVERSATION_TEXT_RE.test(text) && !DATE_GROUP_TEXT_RE.test(text)) {{
        titles.push(text);
      }}
    }};
    document.querySelectorAll('h1, h2, [class*="title" i]').forEach(el => {{
      if (el.offsetParent !== null) add(el.textContent);
    }});
    add((document.title || '').split(/[|-]/)[0]);
    return Array.from(new Set(titles));
  }};
  const findConversationList = () => {{
    const findHistoryRoots = () => {{
      const roots = [];
      const walker = document.createTreeWalker(document.body || document.documentElement, NodeFilter.SHOW_ELEMENT);
      while (walker.nextNode()) {{
        const el = walker.currentNode;
        const text = (el.textContent || '').trim();
        if (text !== '历史对话' && !/^history$/i.test(text)) continue;
        let cursor = el.nextElementSibling;
        while (cursor) {{
          roots.push(cursor);
          cursor = cursor.nextElementSibling;
        }}
      }}
      return roots;
    }};
    const tiers = [
      [
        '[data-testid*="conversation"]',
        '[data-test-id*="conversation"]',
        'a[href^="/c/"]',
        'a[href*="/chat/"]',
        'a[href*="/conversation/"]',
        'a[href*="/thread/"]',
        '[role="listitem"]',
        '[role="option"]',
        'a[href]'
      ],
      [
        'nav a[href^="/c/"]',
        'a[href*="/chat/"]',
        'a[href*="/conversation/"]',
        'a[href*="/thread/"]',
        '[data-testid*="conversation"]',
        '[data-test-id*="conversation"]'
      ],
      [
        "aside a[href]:not([href='#']):not([href='/'])",
        'nav[aria-label*="hist" i] a',
        'nav[aria-label*="chat" i] a',
        'nav[aria-label*="conversation" i] a',
        '[role="navigation"] li a'
      ],
      [
        'aside [role="listitem"]',
        'aside [role="option"]',
        '[role="list"] [role="listitem"]'
      ],
      [
        'aside [class*="conversation" i] [class*="item" i]',
        'aside [class*="history" i] [class*="item" i]',
        'aside [class*="session" i] [class*="item" i]',
        'aside [class*="chat-item" i]',
        'aside [class*="conversationItem" i]',
        'aside [class*="sessionItem" i]',
        'aside [class*="historyItem" i]',
        '[class*="sidebar" i] [class*="item" i]:not([class*="new" i])'
      ]
    ];
    const passes = (el) => {{
      if (!el || el.offsetParent === null) return false;
      const text = (el.textContent || '').trim();
      if (!text) return false;
      if (EXCLUDED_CONVERSATION_TEXT_RE.test(text)) return false;
      if (DATE_GROUP_TEXT_RE.test(text)) return false;
      if (el.closest('header, footer')) return false;
      const href = el.getAttribute && el.getAttribute('href');
      if (href === '/' || href === '#') return false;
      return true;
    }};
    const uniqueConversationItems = (items) => {{
      const seen = new Set();
      const unique = [];
      for (const item of items) {{
        const row = item.closest('a[href], [role="listitem"], [role="option"], li') || item;
        const text = normalizeConversationText(row.textContent);
        const rect = row.getBoundingClientRect();
        const key = text + ':' + Math.round(rect.top);
        if (!text || seen.has(key)) continue;
        seen.add(key);
        unique.push(row);
      }}
      return unique;
    }};
    const historyRoots = findHistoryRoots();
    for (const root of historyRoots) {{
      let combined = [];
      for (const sel of tiers[0]) {{
        try {{
          const found = Array.from(root.querySelectorAll(sel));
          for (const el of found) if (passes(el)) combined.push(el);
        }} catch (_) {{}}
      }}
      combined = uniqueConversationItems(Array.from(new Set(combined)));
      if (combined.length >= 2) return combined;
    }}
    for (const tier of tiers) {{
      let combined = [];
      for (const sel of tier) {{
        try {{
          const found = Array.from(document.querySelectorAll(sel));
          for (const el of found) if (passes(el)) combined.push(el);
        }} catch (_) {{}}
      }}
      if (combined.length < 2) continue;
      combined = uniqueConversationItems(Array.from(new Set(combined)));
      if (combined.length >= 2) return combined;
    }}
    return null;
  }};
  const findCurrentIndex = (items) => {{
    for (let i = 0; i < items.length; i++) {{
      if (isSelectedByStyle(items[i])) return i;
    }}
    const currentTitles = getCurrentConversationTitles();
    if (currentTitles.length) {{
      for (let i = 0; i < items.length; i++) {{
        const text = normalizeConversationText(items[i].textContent);
        if (text && currentTitles.some(title => text.includes(title) || title.includes(text))) return i;
      }}
    }}
    for (let i = 0; i < items.length; i++) {{
      const el = items[i];
      if (el.matches && el.matches('[aria-current], [aria-current="true"], [aria-current="page"], [aria-selected="true"], [data-active="true"]')) return i;
    }}
    for (let i = 0; i < items.length; i++) {{
      const el = items[i];
      const cls = (el.className && typeof el.className === 'string') ? el.className : '';
      if (ACTIVE_CLASS_RE.test(cls)) return i;
      const li = el.closest && el.closest('li, [role="listitem"], [role="option"]');
      if (li && li !== el) {{
        const liCls = (li.className && typeof li.className === 'string') ? li.className : '';
        if (ACTIVE_CLASS_RE.test(liCls)) return i;
        if (li.matches('[aria-current], [aria-selected="true"], [data-active="true"]')) return i;
      }}
    }}
    const path = location.pathname;
    for (let i = 0; i < items.length; i++) {{
      const href = items[i].getAttribute && items[i].getAttribute('href');
      if (!href) continue;
      let clean = '';
      try {{
        clean = new URL(href, location.href).pathname;
      }} catch (_) {{
        clean = href.split('?')[0].split('#')[0];
      }}
      if (clean && clean !== '/' && (path === clean || path.startsWith(clean + '/'))) return i;
    }}
    return -1;
  }};
  window.__polychatSwitchConversationByOffset = (offset) => {{
    const items = findConversationList();
    if (!items || items.length < 2) return false;
    const remembered = window.__polychatLastConversationSwitch;
    let cur = findCurrentIndex(items);
    if (
      remembered &&
      remembered.length === items.length &&
      remembered.index >= 0 &&
      remembered.index < items.length &&
      Date.now() - remembered.at < 10000
    ) {{
      cur = remembered.index;
    }} else if (remembered && remembered.text && Date.now() - remembered.at < 10000) {{
      const rememberedText = normalizeConversationText(remembered.text);
      const rememberedIndex = items.findIndex(item => {{
        const text = normalizeConversationText(item.textContent);
        return text === rememberedText || text.includes(rememberedText) || rememberedText.includes(text);
      }});
      if (rememberedIndex >= 0) cur = rememberedIndex;
    }}
    const next = Number(offset) > 0
      ? (cur < 0 ? 0 : cur + 1)
      : (cur < 0 ? 0 : cur - 1);
    if (next < 0 || next >= items.length) return false;
    window.__polychatLastConversationSwitch = {{
      index: next,
      length: items.length,
      text: normalizeConversationText(items[next].textContent),
      at: Date.now()
    }};
    try {{ items[next].scrollIntoView({{ block: 'nearest' }}); }} catch (_) {{}}
    const target = items[next].matches('a, button, [role="button"], [tabindex]') ? items[next] : (items[next].querySelector('a, button, [role="button"], [tabindex]') || items[next]);
    target.click();
    return true;
  }};
  document.addEventListener('keydown', (event) => {{
    if (!(event.metaKey || event.ctrlKey)) return;
    const rawKey = String(event.key || '');
    const lower = rawKey.toLowerCase();
    if (lower === 'f') {{
      event.preventDefault();
      event.stopPropagation();
      openFindBox();
      return;
    }}
    if (lower === 'q') {{
      event.preventDefault();
      event.stopPropagation();
      quitApp();
      return;
    }}
    if (/^[1-9]$/.test(rawKey)) {{
      event.preventDefault();
      event.stopPropagation();
      dispatchShortcut('switch-platform', {{ index: parseInt(rawKey, 10) }});
      return;
    }}
    if (rawKey === 'Tab') {{
      event.preventDefault();
      event.stopPropagation();
      dispatchShortcut('switch-tab', {{ offset: event.shiftKey ? -1 : 1 }});
      return;
    }}
    const isPrevConversationShortcut = event.shiftKey && (event.code === 'BracketLeft' || rawKey === '[' || rawKey === '{{');
    const isNextConversationShortcut = event.shiftKey && (event.code === 'BracketRight' || rawKey === ']' || rawKey === '}}');
    if (isPrevConversationShortcut || isNextConversationShortcut) {{
      if (event.isComposing || event.keyCode === 229) return;
      const switched = window.__polychatSwitchConversationByOffset(isNextConversationShortcut ? 1 : -1);
      if (!switched) return;
      event.preventDefault();
      event.stopPropagation();
      return;
    }}
  }}, true);
}})();
"#
  )
}

fn conversation_switch_script(offset: i32) -> String {
  let normalized_offset = if offset >= 0 { 1 } else { -1 };
  format!(
    r#"
(() => {{
  const offset = {normalized_offset};
  if (window.__polychatSwitchConversationByOffset?.(offset)) return;

  const EXCLUDED_CONVERSATION_TEXT_RE = /^(new chat|new conversation|新建|新对话|新会话|新聊天|ai 创作|ai创作|云盘|更多|我的创作)$/i;
  const DATE_GROUP_TEXT_RE = /^\d{{4}}(?:[-/年]\s*)?(?:0?[1-9]|1[0-2])月?$/;
  const ACTIVE_CLASS_RE = /(?:^|[\s_-])(active|selected|current|is-active|is-selected)(?:$|[\s_-])/;
  const normalizeConversationText = (text) => String(text || '').replace(/[⋯…]/g, '').replace(/\s+/g, ' ').trim();
  const isSelectedByStyle = (el) => {{
    let cursor = el;
    for (let depth = 0; cursor && depth < 4; depth++, cursor = cursor.parentElement) {{
      const style = getComputedStyle(cursor);
      const color = style.backgroundColor || '';
      const match = color.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/i);
      if (!match) continue;
      const r = Number(match[1]);
      const g = Number(match[2]);
      const b = Number(match[3]);
      if (b > r + 15 && b >= g + 5) return true;
    }}
    return false;
  }};
  const getCurrentConversationTitles = () => {{
    const titles = [];
    const add = (value) => {{
      const text = normalizeConversationText(value);
      if (text.length >= 3 && text.length <= 120 && !EXCLUDED_CONVERSATION_TEXT_RE.test(text) && !DATE_GROUP_TEXT_RE.test(text)) {{
        titles.push(text);
      }}
    }};
    document.querySelectorAll('h1, h2, [class*="title" i]').forEach(el => {{
      if (el.offsetParent !== null) add(el.textContent);
    }});
    add((document.title || '').split(/[|-]/)[0]);
    return Array.from(new Set(titles));
  }};
  const findConversationList = () => {{
    const findHistoryRoots = () => {{
      const roots = [];
      const walker = document.createTreeWalker(document.body || document.documentElement, NodeFilter.SHOW_ELEMENT);
      while (walker.nextNode()) {{
        const el = walker.currentNode;
        const text = (el.textContent || '').trim();
        if (text !== '历史对话' && !/^history$/i.test(text)) continue;
        let cursor = el.nextElementSibling;
        while (cursor) {{
          roots.push(cursor);
          cursor = cursor.nextElementSibling;
        }}
      }}
      return roots;
    }};
    const tiers = [
      [
        '[data-testid*="conversation"]',
        '[data-test-id*="conversation"]',
        'a[href^="/c/"]',
        'a[href*="/chat/"]',
        'a[href*="/conversation/"]',
        'a[href*="/thread/"]',
        '[role="listitem"]',
        '[role="option"]',
        'a[href]'
      ],
      [
        'nav a[href^="/c/"]',
        'a[href*="/chat/"]',
        'a[href*="/conversation/"]',
        'a[href*="/thread/"]',
        '[data-testid*="conversation"]',
        '[data-test-id*="conversation"]'
      ],
      [
        "aside a[href]:not([href='#']):not([href='/'])",
        'nav[aria-label*="hist" i] a',
        'nav[aria-label*="chat" i] a',
        'nav[aria-label*="conversation" i] a',
        '[role="navigation"] li a'
      ],
      [
        'aside [role="listitem"]',
        'aside [role="option"]',
        '[role="list"] [role="listitem"]'
      ],
      [
        'aside [class*="conversation" i] [class*="item" i]',
        'aside [class*="history" i] [class*="item" i]',
        'aside [class*="session" i] [class*="item" i]',
        'aside [class*="chat-item" i]',
        'aside [class*="conversationItem" i]',
        'aside [class*="sessionItem" i]',
        'aside [class*="historyItem" i]',
        '[class*="sidebar" i] [class*="item" i]:not([class*="new" i])'
      ]
    ];
    const passes = (el) => {{
      if (!el || el.offsetParent === null) return false;
      const text = (el.textContent || '').trim();
      if (!text) return false;
      if (EXCLUDED_CONVERSATION_TEXT_RE.test(text)) return false;
      if (DATE_GROUP_TEXT_RE.test(text)) return false;
      if (el.closest('header, footer')) return false;
      const href = el.getAttribute && el.getAttribute('href');
      if (href === '/' || href === '#') return false;
      return true;
    }};
    const uniqueConversationItems = (items) => {{
      const seen = new Set();
      const unique = [];
      for (const item of items) {{
        const row = item.closest('a[href], [role="listitem"], [role="option"], li') || item;
        const text = normalizeConversationText(row.textContent);
        const rect = row.getBoundingClientRect();
        const key = text + ':' + Math.round(rect.top);
        if (!text || seen.has(key)) continue;
        seen.add(key);
        unique.push(row);
      }}
      return unique;
    }};
    const historyRoots = findHistoryRoots();
    for (const root of historyRoots) {{
      let combined = [];
      for (const sel of tiers[0]) {{
        try {{
          const found = Array.from(root.querySelectorAll(sel));
          for (const el of found) if (passes(el)) combined.push(el);
        }} catch (_) {{}}
      }}
      combined = uniqueConversationItems(Array.from(new Set(combined)));
      if (combined.length >= 2) return combined;
    }}
    for (const tier of tiers) {{
      let combined = [];
      for (const sel of tier) {{
        try {{
          const found = Array.from(document.querySelectorAll(sel));
          for (const el of found) if (passes(el)) combined.push(el);
        }} catch (_) {{}}
      }}
      if (combined.length < 2) continue;
      combined = uniqueConversationItems(Array.from(new Set(combined)));
      if (combined.length >= 2) return combined;
    }}
    return null;
  }};
  const findCurrentIndex = (items) => {{
    for (let i = 0; i < items.length; i++) {{
      if (isSelectedByStyle(items[i])) return i;
    }}
    const currentTitles = getCurrentConversationTitles();
    if (currentTitles.length) {{
      for (let i = 0; i < items.length; i++) {{
        const text = normalizeConversationText(items[i].textContent);
        if (text && currentTitles.some(title => text.includes(title) || title.includes(text))) return i;
      }}
    }}
    for (let i = 0; i < items.length; i++) {{
      const el = items[i];
      if (el.matches && el.matches('[aria-current], [aria-current="true"], [aria-current="page"], [aria-selected="true"], [data-active="true"]')) return i;
    }}
    for (let i = 0; i < items.length; i++) {{
      const el = items[i];
      const cls = (el.className && typeof el.className === 'string') ? el.className : '';
      if (ACTIVE_CLASS_RE.test(cls)) return i;
      const li = el.closest && el.closest('li, [role="listitem"], [role="option"]');
      if (li && li !== el) {{
        const liCls = (li.className && typeof li.className === 'string') ? li.className : '';
        if (ACTIVE_CLASS_RE.test(liCls)) return i;
        if (li.matches('[aria-current], [aria-selected="true"], [data-active="true"]')) return i;
      }}
    }}
    const path = location.pathname;
    for (let i = 0; i < items.length; i++) {{
      const href = items[i].getAttribute && items[i].getAttribute('href');
      if (!href) continue;
      let clean = '';
      try {{
        clean = new URL(href, location.href).pathname;
      }} catch (_) {{
        clean = href.split('?')[0].split('#')[0];
      }}
      if (clean && clean !== '/' && (path === clean || path.startsWith(clean + '/'))) return i;
    }}
    return -1;
  }};
  const items = findConversationList();
  if (!items || items.length < 2) return;
  const remembered = window.__polychatLastConversationSwitch;
  let cur = findCurrentIndex(items);
  if (
    remembered &&
    remembered.length === items.length &&
    remembered.index >= 0 &&
    remembered.index < items.length &&
    Date.now() - remembered.at < 10000
  ) {{
    cur = remembered.index;
  }} else if (remembered && remembered.text && Date.now() - remembered.at < 10000) {{
    const rememberedText = normalizeConversationText(remembered.text);
    const rememberedIndex = items.findIndex(item => {{
      const text = normalizeConversationText(item.textContent);
      return text === rememberedText || text.includes(rememberedText) || rememberedText.includes(text);
    }});
    if (rememberedIndex >= 0) cur = rememberedIndex;
  }}
  const next = offset > 0
    ? (cur < 0 ? 0 : cur + 1)
    : (cur < 0 ? 0 : cur - 1);
  if (next < 0 || next >= items.length) return;
  window.__polychatLastConversationSwitch = {{
    index: next,
    length: items.length,
    text: normalizeConversationText(items[next].textContent),
    at: Date.now()
  }};
  try {{ items[next].scrollIntoView({{ block: 'nearest' }}); }} catch (_) {{}}
  const target = items[next].matches('a, button, [role="button"], [tabindex]') ? items[next] : (items[next].querySelector('a, button, [role="button"], [tabindex]') || items[next]);
  target.click();
}})();
"#
  )
}

#[tauri::command]
fn create_platform_view(
  app: AppHandle,
  window: Window,
  views: tauri::State<'_, PlatformViews>,
  platform_id: String,
  platform_name: String,
  url: String,
  bounds: ViewBounds,
  user_agent: Option<String>,
  storage_id: Option<String>,
) -> Result<PlatformState, String> {
  if let Some((webview, state)) = {
    let views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
    views
      .get(&platform_id)
      .map(|view| (view.webview.clone(), get_state_payload(&platform_id, view)))
  } {
    webview
      .set_bounds(rect_from_bounds(&window, bounds.clone()))
      .map_err(|err| err.to_string())?;
    return Ok(state);
  }

  let parsed_url = Url::parse(&url).map_err(|err| format!("invalid url: {err}"))?;
  let platform_label = format!("platform-{}", sanitize_platform_id(&platform_id));
  let storage_id = storage_id.unwrap_or_else(|| platform_id.clone());
  let data_dir = app
    .path()
    .app_data_dir()
    .map_err(|err| format!("failed to resolve app data dir: {err}"))?
    .join("platforms")
    .join(sanitize_platform_id(&storage_id));
  fs::create_dir_all(&data_dir)
    .map_err(|err| format!("failed to create platform data dir: {err}"))?;

  let app_for_load = app.clone();
  let platform_for_load = platform_id.clone();
  let app_for_new_window = app.clone();
  let platform_for_new_window = platform_id.clone();
  let storage_for_new_window = storage_id.clone();
  let app_for_download = app.clone();
  let platform_for_download = platform_id.clone();
  // 在 Requested 阶段记录目标路径，Finished 阶段回填（macOS 上 wry 不会回传路径）
  let download_destinations: Arc<Mutex<HashMap<String, PathBuf>>> =
    Arc::new(Mutex::new(HashMap::new()));
  let download_dests_for_started = download_destinations.clone();
  let download_dests_for_finished = download_destinations.clone();
  let initial_title = platform_name.clone();
  let initial_url = url.clone();

  let effective_user_agent = user_agent.unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());
  let builder = WebviewBuilder::new(platform_label, WebviewUrl::External(parsed_url))
    .user_agent(&effective_user_agent)
    .devtools(cfg!(debug_assertions))
    .data_store_identifier(data_store_identifier(&storage_id))
    .data_directory(data_dir)
    // 在页面 JS 执行前最早注入：修平 WebView 上的 navigator 自动化指纹
    .initialization_script(stealth_init_script())
    .on_download(move |_webview, event| match event {
      DownloadEvent::Requested { url, destination } => {
        eprintln!("[polychat] download Requested: {}", url);
        // 把目标路径定到系统 ~/Downloads/<文件名>，同名追加 -1/-2…
        let dir = app_for_download
          .path()
          .download_dir()
          .or_else(|_| app_for_download.path().home_dir().map(|h| h.join("Downloads")))
          .unwrap_or_else(|_| PathBuf::from("."));
        let filename = preferred_download_filename(&url, destination);
        let target = unique_path(dir.join(filename));
        eprintln!("[polychat] download destination: {}", target.display());
        if let Ok(mut map) = download_dests_for_started.lock() {
          map.insert(url.to_string(), target.clone());
        }
        *destination = target;
        true
      }
      DownloadEvent::Finished { url, path, success } => {
        eprintln!(
          "[polychat] download Finished: url={} path={:?} success={}",
          url, path, success
        );
        // macOS 上 wry 的 path 一直为 None，用 Requested 时缓存的目标路径回填
        let resolved_path = path.or_else(|| {
          download_dests_for_finished
            .lock()
            .ok()
            .and_then(|mut m| m.remove(url.as_str()))
        });
        let filename = resolved_path
          .as_ref()
          .and_then(|p| p.file_name())
          .and_then(|n| n.to_str())
          .map(|s| s.to_string())
          .or_else(|| Some(guess_download_filename(&url)));
        let _ = app_for_download.emit(
          "platform-download-finished",
          DownloadFinishedEvent {
            platform_id: platform_for_download.clone(),
            url: url.to_string(),
            filename,
            path: resolved_path.and_then(|p| p.to_str().map(|s| s.to_string())),
            success,
          },
        );
        true
      }
      _ => true,
    })
    .on_new_window(move |url, _features| {
      // 仅对跨域 http(s) 弹窗转为新标签；同源弹窗（OAuth/CF 挑战等）保持原生行为，
      // 避免破坏依赖 window 引用的回调通信。
      let parent_url = app_for_new_window
        .try_state::<PlatformViews>()
        .and_then(|views| {
          views
            .0
            .lock()
            .ok()
            .and_then(|views| views.get(&platform_for_new_window).map(|v| v.state.url.clone()))
        })
        .unwrap_or_default();

      if is_auth_popup_url(&url) {
        NewWindowResponse::Allow
      } else if is_external_http_navigation(&url, &parent_url) {
        emit_open_tab_request(
          &app_for_new_window,
          &storage_for_new_window,
          &platform_for_new_window,
          url.as_str(),
        );
        NewWindowResponse::Deny
      } else {
        NewWindowResponse::Allow
      }
    })
    .on_page_load(move |webview, payload| {
      let app_handle = app_for_load.clone();
      let platform_id = platform_for_load.clone();
      let tab_js = tab_interceptor_script(&storage_id, &platform_id);
      let title_js = "window.__POLYCHAT_TITLE_POLL__&&clearInterval(window.__POLYCHAT_TITLE_POLL__);window.__POLYCHAT_TITLE_POLL__=setInterval(()=>{window.__TAURI_INTERNALS__?.invoke('update_platform_title',{platformId:'".to_string()
        + &platform_id
        + "',title:document.title||'',url:location.href||'',canGoBack:history.length>1,canGoForward:false})},1000);window.__TAURI_INTERNALS__?.invoke('update_platform_title',{platformId:'"
        + &platform_id
        + "',title:document.title||'',url:location.href||'',canGoBack:history.length>1,canGoForward:false});";
      let _ = webview.eval(&tab_js);
      let _ = webview.eval(&title_js);
      let loading = matches!(payload.event(), tauri::webview::PageLoadEvent::Started);
      if let Some(views) = app_handle.try_state::<PlatformViews>() {
        if let Ok(mut views) = views.0.lock() {
          if let Some(view) = views.get_mut(&platform_id) {
            view.state.loading = loading;
            view.state.url = payload.url().to_string();
            emit_platform_state(&app_handle, &platform_id, &view.state);
          }
        }
      }
    });

  let webview = window
    .add_child(
      builder,
      physical_position_from_bounds(&window, &bounds),
      physical_size_from_bounds(&window, &bounds),
    )
    .map_err(|err| format!("failed to create webview: {err}"))?;
  webview
    .show()
    .map_err(|err| format!("failed to show webview: {err}"))?;

  let state = PlatformViewState {
    title: initial_title,
    can_go_back: false,
    can_go_forward: false,
    loading: true,
    url: initial_url,
  };

  emit_platform_state(&app, &platform_id, &state);
  let mut views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
  views.insert(platform_id.clone(), PlatformView { webview, state });
  Ok(get_state_payload(&platform_id, views.get(&platform_id).unwrap()))
}

fn get_state_payload(platform_id: &str, view: &PlatformView) -> PlatformState {
  PlatformState {
    platform_id: platform_id.to_string(),
    title: view.state.title.clone(),
    can_go_back: view.state.can_go_back,
    can_go_forward: view.state.can_go_forward,
    loading: view.state.loading,
    url: view.state.url.clone(),
  }
}

#[tauri::command]
fn show_platform_view(
  views: tauri::State<'_, PlatformViews>,
  platform_id: String,
) -> Result<(), String> {
  let webviews = {
    let views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
    views
      .iter()
      .map(|(id, view)| (id == &platform_id, view.webview.clone()))
      .collect::<Vec<_>>()
  };

  for (is_active, webview) in webviews {
    if is_active {
      webview.show().map_err(|err| err.to_string())?;
      // 把焦点显式交给新显示的 WebView，避免按键事件继续投递到刚被隐藏的 WebView，
      // 否则在 macOS 下连续触发快捷键时会"丢键"。
      let _ = webview.set_focus();
    } else {
      webview.hide().map_err(|err| err.to_string())?;
    }
  }
  Ok(())
}

/// 分屏模式：同时显示 platform_ids 中的所有 WebView，隐藏其余。
/// 只把焦点交给集合中的第一个（primary），避免多个 WebView 互相抢焦点。
#[tauri::command]
fn show_platform_views(
  views: tauri::State<'_, PlatformViews>,
  platform_ids: Vec<String>,
) -> Result<(), String> {
  let webviews = {
    let views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
    views
      .iter()
      .map(|(id, view)| (platform_ids.contains(id), view.webview.clone()))
      .collect::<Vec<_>>()
  };

  let primary = platform_ids.first().cloned();
  let primary_webview = {
    let views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
    primary
      .as_ref()
      .and_then(|id| views.get(id).map(|view| view.webview.clone()))
  };

  for (is_visible, webview) in webviews {
    if is_visible {
      webview.show().map_err(|err| err.to_string())?;
    } else {
      webview.hide().map_err(|err| err.to_string())?;
    }
  }

  if let Some(webview) = primary_webview {
    let _ = webview.set_focus();
  }
  Ok(())
}

#[tauri::command]
fn close_platform_view(
  views: tauri::State<'_, PlatformViews>,
  platform_id: String,
) -> Result<(), String> {
  let webview = {
    let mut views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
    views.remove(&platform_id).map(|view| view.webview)
  };

  if let Some(webview) = webview {
    webview.close().map_err(|err| err.to_string())?;
  }
  Ok(())
}

#[tauri::command]
fn hide_all_platform_views(views: tauri::State<'_, PlatformViews>) -> Result<(), String> {
  let webviews = {
    let views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
    views
      .values()
      .map(|view| view.webview.clone())
      .collect::<Vec<_>>()
  };

  for webview in webviews {
    webview.hide().map_err(|err| err.to_string())?;
  }
  Ok(())
}

#[tauri::command]
fn set_platform_view_bounds(
  window: Window,
  views: tauri::State<'_, PlatformViews>,
  platform_id: String,
  bounds: ViewBounds,
) -> Result<(), String> {
  let webview = {
    let views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
    views.get(&platform_id).map(|view| view.webview.clone())
  };

  if let Some(webview) = webview {
    webview
      .set_bounds(rect_from_bounds(&window, bounds))
      .map_err(|err| err.to_string())?;
  }
  Ok(())
}

#[tauri::command]
fn navigate(
  views: tauri::State<'_, PlatformViews>,
  platform_id: String,
  action: String,
) -> Result<(), String> {
  let script = match action.as_str() {
    "back" => "history.back()",
    "forward" => "history.forward()",
    "reload" => "location.reload()",
    _ => return Err(format!("unknown navigation action: {action}")),
  };

  let webview = {
    let mut views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
    let Some(view) = views.get_mut(&platform_id) else {
      return Ok(());
    };
    view.state.loading = true;
    view.webview.clone()
  };

  webview.eval(script).map_err(|err| err.to_string())?;
  Ok(())
}

#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
  open_url_with_system_browser(&url)
}

#[tauri::command]
fn clear_platform_data(
  app: AppHandle,
  views: tauri::State<'_, PlatformViews>,
  platform_id: String,
) -> Result<(), String> {
  let sanitized_id = sanitize_platform_id(&platform_id);
  let webview = {
    let views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
    views.get(&platform_id).map(|view| view.webview.clone())
  };

  if let Some(webview) = webview {
    let _ = webview.eval("localStorage.clear();sessionStorage.clear();location.reload()");
    let _ = webview.clear_all_browsing_data();
  }

  let data_dir = app
    .path()
    .app_data_dir()
    .map_err(|err| format!("failed to resolve app data dir: {err}"))?
    .join("platforms")
    .join(sanitized_id);
  if data_dir.exists() {
    fs::remove_dir_all(data_dir).map_err(|err| format!("failed to clear platform data: {err}"))?;
  }
  Ok(())
}

#[tauri::command]
fn get_platform_state(
  views: tauri::State<'_, PlatformViews>,
  platform_id: String,
) -> Result<PlatformState, String> {
  let views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
  let Some(view) = views.get(&platform_id) else {
    return Err(format!("platform view not found: {platform_id}"));
  };
  Ok(get_state_payload(&platform_id, view))
}

#[tauri::command]
fn update_platform_title(
  app: AppHandle,
  views: tauri::State<'_, PlatformViews>,
  platform_id: String,
  title: String,
  url: String,
  can_go_back: bool,
  can_go_forward: bool,
) -> Result<(), String> {
  let mut views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
  let Some(view) = views.get_mut(&platform_id) else {
    return Ok(());
  };

  view.state.title = if title.trim().is_empty() {
    platform_id.clone()
  } else {
    title
  };
  view.state.url = url;
  view.state.can_go_back = can_go_back;
  view.state.can_go_forward = can_go_forward;
  view.state.loading = false;
  emit_platform_state(&app, &platform_id, &view.state);
  Ok(())
}

#[tauri::command]
fn open_platform_tab(
  app: AppHandle,
  platform_id: String,
  opener_view_id: String,
  url: String,
) -> Result<(), String> {
  emit_open_tab_request(&app, &platform_id, &opener_view_id, &url);
  Ok(())
}

#[tauri::command]
fn quit_app(app: AppHandle) {
  app.exit(0);
}

#[tauri::command]
fn dispatch_shortcut(
  app: AppHandle,
  action: String,
  index: Option<u32>,
  offset: Option<i32>,
) -> Result<(), String> {
  let _ = app.emit(
    "polychat-shortcut",
    ShortcutEvent {
      action,
      index,
      offset,
    },
  );
  Ok(())
}

#[tauri::command]
fn switch_conversation(
  views: tauri::State<'_, PlatformViews>,
  platform_id: String,
  offset: i32,
) -> Result<(), String> {
  eprintln!(
    "[polychat] switch_conversation: platform={} offset={}",
    platform_id, offset
  );
  let webview = {
    let views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
    views.get(&platform_id).map(|view| view.webview.clone())
  }
  .ok_or_else(|| format!("platform view not found: {platform_id}"))?;

  webview
    .eval(conversation_switch_script(offset))
    .map_err(|err| err.to_string())?;
  Ok(())
}

/// 生成「把文本填充进网页输入框」的注入脚本。
/// `json_text` 必须是已经过 serde_json 编码的安全 JS 字符串字面量（含两端引号）。
/// 按 brand 选择候选选择器，填充方式根据元素类型自适应：
///   - textarea / input：用原型 value setter + dispatch input 事件（兼容 React 受控组件）
///   - contenteditable：focus + 全选 + execCommand('insertText')
/// 仅填充，不触发发送。找不到元素时调用 debug_log，静默不抛错。
fn fill_input_script(brand: &str, json_text: &str) -> String {
  // 每个 brand 的候选选择器（best-effort，从精确到宽松回退）
  let selectors: &[&str] = match brand {
    "chatgpt" => &[
      "#prompt-textarea",
      "div.ProseMirror[contenteditable=\"true\"]",
      "[contenteditable=\"true\"]",
    ],
    "claude" => &[
      "div.ProseMirror[contenteditable=\"true\"]",
      "[contenteditable=\"true\"]",
    ],
    "deepseek" => &[
      "textarea#chat-input",
      "textarea[placeholder]",
      "textarea",
    ],
    "doubao" => &[
      "textarea[data-testid*=\"chat_input\"]",
      "textarea",
      "[contenteditable=\"true\"]",
    ],
    "qwen" => &[
      "textarea#chat-input",
      "textarea[placeholder]",
      "textarea",
      "[contenteditable=\"true\"]",
    ],
    _ => &[
      "textarea",
      "[contenteditable=\"true\"]",
      "input[type=\"text\"]",
    ],
  };

  let selectors_js = selectors
    .iter()
    .map(|s| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string()))
    .collect::<Vec<_>>()
    .join(",");

  let brand_js = serde_json::to_string(brand).unwrap_or_else(|_| "\"\"".to_string());

  format!(
    r#"(function() {{
  var text = {json_text};
  var selectors = [{selectors_js}];
  var brand = {brand_js};
  function visible(el) {{ return el && el.offsetParent !== null; }}
  function pick() {{
    for (var i = 0; i < selectors.length; i++) {{
      try {{
        var list = document.querySelectorAll(selectors[i]);
        for (var j = 0; j < list.length; j++) {{
          if (visible(list[j])) return list[j];
        }}
      }} catch (e) {{}}
    }}
    return null;
  }}
  function report(tag) {{
    try {{
      window.__TAURI_INTERNALS__.invoke('debug_log', {{ tag: tag, payload: brand }});
    }} catch (e) {{}}
  }}
  function sendEnter(el) {{
    function fire(type) {{
      el.dispatchEvent(new KeyboardEvent(type, {{
        key: 'Enter', code: 'Enter', keyCode: 13, which: 13,
        bubbles: true, cancelable: true
      }}));
    }}
    fire('keydown');
    fire('keypress');
    fire('keyup');
  }}
  try {{
    var el = pick();
    if (!el) {{ report('fill-miss'); return; }}
    var tag = (el.tagName || '').toLowerCase();
    var filled = false;
    if (tag === 'textarea' || tag === 'input') {{
      var proto = tag === 'textarea'
        ? window.HTMLTextAreaElement.prototype
        : window.HTMLInputElement.prototype;
      var setter = Object.getOwnPropertyDescriptor(proto, 'value').set;
      el.focus();
      setter.call(el, text);
      el.dispatchEvent(new Event('input', {{ bubbles: true }}));
      filled = true;
    }} else if (el.isContentEditable) {{
      el.focus();
      var sel = window.getSelection();
      sel.selectAllChildren(el);
      var ok = document.execCommand('insertText', false, text);
      if (!ok) {{
        el.dispatchEvent(new InputEvent('beforeinput', {{
          inputType: 'insertText', data: text, bubbles: true, cancelable: true
        }}));
      }}
      filled = true;
    }} else {{
      report('fill-unknown-el');
    }}
    if (filled) {{
      // 延时再发送，给 React 受控组件的 value 更新留出时间
      setTimeout(function() {{
        try {{ sendEnter(el); }} catch (e) {{ report('send-error'); }}
      }}, 120);
    }}
  }} catch (e) {{
    report('fill-error');
  }}
}})();"#
  )
}

/// 广播填充：把 text 填入指定平台 WebView 的输入框（不触发发送）。
/// brand 用于分派各平台不同的 DOM 选择器（前端显式传入，跨 clone/重定向稳定）。
#[tauri::command]
fn fill_platform_input(
  views: tauri::State<'_, PlatformViews>,
  platform_id: String,
  brand: String,
  text: String,
) -> Result<(), String> {
  let webview = {
    let views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
    views.get(&platform_id).map(|view| view.webview.clone())
  }
  .ok_or_else(|| format!("platform view not found: {platform_id}"))?;

  // 文本经 serde_json 编码为安全 JS 字符串字面量，杜绝 JS 注入
  let json_text = serde_json::to_string(&text).map_err(|err| err.to_string())?;
  webview
    .eval(fill_input_script(&brand, &json_text))
    .map_err(|err| err.to_string())?;
  Ok(())
}

/// 注入脚本里的诊断 sink：当 clipboard hook 或其它注入逻辑命中错误分支时，
/// 把上下文打到 stderr 方便排查（只有失败路径调用，正常使用零开销）。
#[tauri::command]
fn debug_log(tag: String, payload: String) {
  eprintln!("[polychat][probe][{}] {}", tag, payload);
}

/// 把 base64 编码的图片（PNG/JPEG/GIF 等任意 image crate 支持的格式）写入系统剪贴板。
/// 解决 macOS WKWebView 拒绝 `navigator.clipboard.write(ClipboardItem)` 写图片的限制：
/// 注入脚本拦截网页 clipboard.write，把图片二进制 base64 后传给本命令，由原生 arboard 写入。
#[tauri::command]
fn copy_image_to_clipboard(data_base64: String) -> Result<(), String> {
  use base64_decode_compat as decode;
  let bytes = decode(&data_base64).map_err(|e| format!("invalid base64: {e}"))?;
  // arboard 要求 RGBA8 原始像素，所以先用 image crate 解码再喂进去
  let img = image::load_from_memory(&bytes).map_err(|e| format!("decode image failed: {e}"))?;
  let rgba = img.to_rgba8();
  let (width, height) = rgba.dimensions();
  let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("clipboard open: {e}"))?;
  clipboard
    .set_image(arboard::ImageData {
      width: width as usize,
      height: height as usize,
      bytes: std::borrow::Cow::Owned(rgba.into_raw()),
    })
    .map_err(|e| format!("clipboard set_image: {e}"))?;
  eprintln!(
    "[polychat] copy_image_to_clipboard ok: {}x{} bytes={}",
    width,
    height,
    bytes.len()
  );
  Ok(())
}

/// 接收来自注入脚本的 blob/data 下载请求。
/// JS 端拦截了 a[download] 的点击、把内容读成 base64 传过来；
/// 我们解码后写入系统 Downloads 目录，再发与原生下载同形态的完成事件。
#[tauri::command]
fn save_download_blob(
  app: AppHandle,
  platform_id: String,
  filename: Option<String>,
  source_url: Option<String>,
  data_base64: String,
) -> Result<(), String> {
  use base64_decode_compat as decode;

  let bytes = decode(&data_base64).map_err(|e| format!("invalid base64: {e}"))?;
  let dir = app
    .path()
    .download_dir()
    .or_else(|_| app.path().home_dir().map(|h| h.join("Downloads")))
    .unwrap_or_else(|_| PathBuf::from("."));
  let fname = filename
    .filter(|s| !s.trim().is_empty())
    .unwrap_or_else(|| "download".to_string());
  let target = unique_path(dir.join(sanitize_filename(&fname)));
  let success = fs::write(&target, &bytes).is_ok();
  eprintln!(
    "[polychat] save_download_blob: platform={} file={} success={}",
    platform_id,
    target.display(),
    success
  );
  let _ = app.emit(
    "platform-download-finished",
    DownloadFinishedEvent {
      platform_id,
      url: source_url.unwrap_or_default(),
      filename: target.file_name().and_then(|s| s.to_str()).map(|s| s.to_string()),
      path: target.to_str().map(|s| s.to_string()),
      success,
    },
  );
  Ok(())
}

/// JS 端拦截到下载、但读取/转码失败时调用，向前端发同形态的失败事件。
/// 保证用户至少看到一次 toast，不会"点了没反应"。
#[tauri::command]
fn report_download_error(
  app: AppHandle,
  platform_id: String,
  filename: Option<String>,
  source_url: Option<String>,
  reason: Option<String>,
) -> Result<(), String> {
  let display_name = filename
    .clone()
    .filter(|s| !s.trim().is_empty())
    .unwrap_or_else(|| "download".to_string());
  eprintln!(
    "[polychat] download failed: platform={} file={} reason={}",
    platform_id,
    display_name,
    reason.as_deref().unwrap_or("unknown")
  );
  let _ = app.emit(
    "platform-download-finished",
    DownloadFinishedEvent {
      platform_id,
      url: source_url.unwrap_or_default(),
      filename: Some(display_name),
      path: None,
      success: false,
    },
  );
  Ok(())
}

fn sanitize_filename(name: &str) -> String {
  name
    .chars()
    .map(|c| match c {
      '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
      _ => c,
    })
    .collect()
}

/// 极简 base64 解码（不引入额外依赖）。
fn base64_decode_compat(s: &str) -> Result<Vec<u8>, String> {
  // 容忍换行/空白
  let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
  // 去掉 data URL 前缀，例如 data:application/pdf;base64,xxxx
  let payload = if let Some(idx) = cleaned.find(";base64,") {
    &cleaned[idx + ";base64,".len()..]
  } else {
    cleaned.as_str()
  };

  const CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let mut lookup = [255u8; 256];
  for (i, &b) in CHARS.iter().enumerate() {
    lookup[b as usize] = i as u8;
  }
  lookup[b'-' as usize] = 62; // url-safe
  lookup[b'_' as usize] = 63;

  let bytes = payload.as_bytes();
  let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
  let mut buf = [0u8; 4];
  let mut idx = 0;
  for &b in bytes {
    if b == b'=' {
      break;
    }
    let v = lookup[b as usize];
    if v == 255 {
      return Err(format!("invalid base64 char: {b}"));
    }
    buf[idx] = v;
    idx += 1;
    if idx == 4 {
      out.push((buf[0] << 2) | (buf[1] >> 4));
      out.push((buf[1] << 4) | (buf[2] >> 2));
      out.push((buf[2] << 6) | buf[3]);
      idx = 0;
    }
  }
  if idx == 2 {
    out.push((buf[0] << 2) | (buf[1] >> 4));
  } else if idx == 3 {
    out.push((buf[0] << 2) | (buf[1] >> 4));
    out.push((buf[1] << 4) | (buf[2] >> 2));
  }
  Ok(out)
}

fn build_app_menu<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<Menu<R>> {
  let menu = Menu::default(app.handle())?;
  let prev_conversation = MenuItem::with_id(
    app,
    MENU_ID_PREV_CONVERSATION,
    "Previous Conversation",
    true,
    Some("CmdOrCtrl+Shift+["),
  )?;
  let next_conversation = MenuItem::with_id(
    app,
    MENU_ID_NEXT_CONVERSATION,
    "Next Conversation",
    true,
    Some("CmdOrCtrl+Shift+]"),
  )?;
  let polychat_menu = Submenu::with_id_and_items(
    app,
    "polychat-actions",
    "PolyChat",
    true,
    &[&prev_conversation, &next_conversation],
  )?;
  menu.append(&polychat_menu)?;
  Ok(menu)
}

pub fn run() {
  tauri::Builder::default()
    .manage(PlatformViews::default())
    .setup(|app| {
      app.set_menu(build_app_menu(app)?)?;
      Ok(())
    })
    .on_menu_event(|app, event| {
      let id = event.id().0.as_str();
      if id == "quit" {
        app.exit(0);
      } else if id == MENU_ID_PREV_CONVERSATION {
        eprintln!("[polychat] menu shortcut switch-conversation offset=-1");
        let _ = app.emit(
          "polychat-shortcut",
          ShortcutEvent {
            action: "switch-conversation".to_string(),
            index: None,
            offset: Some(-1),
          },
        );
      } else if id == MENU_ID_NEXT_CONVERSATION {
        eprintln!("[polychat] menu shortcut switch-conversation offset=1");
        let _ = app.emit(
          "polychat-shortcut",
          ShortcutEvent {
            action: "switch-conversation".to_string(),
            index: None,
            offset: Some(1),
          },
        );
      }
    })
    .invoke_handler(tauri::generate_handler![
      create_platform_view,
      show_platform_view,
      show_platform_views,
      close_platform_view,
      hide_all_platform_views,
      set_platform_view_bounds,
      navigate,
      fill_platform_input,
      open_external,
      clear_platform_data,
      get_platform_state,
      update_platform_title,
      open_platform_tab,
      quit_app,
      dispatch_shortcut,
      switch_conversation,
      debug_log,
      copy_image_to_clipboard,
      save_download_blob,
      report_download_error
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
