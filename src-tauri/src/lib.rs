use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
#[cfg(target_os = "windows")]
use std::thread;
use tauri::menu::{Menu, MenuItem, Submenu};
use tauri::webview::{DownloadEvent, NewWindowResponse};
use tauri::{
  AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Rect,
  Runtime, Webview, WebviewBuilder, WebviewUrl, Window,
};
use url::Url;

#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_CONTROL, MOD_SHIFT};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
  GetForegroundWindow, GetMessageW, MSG, WM_HOTKEY,
};

// macOS 娑?Tauri 娴ｈ法鏁ょ化鑽ょ埠 WKWebView閿涘牅绗?Safari 閸氬苯绱╅幙搴礆閿涘A 韫囧懘銆忔稉搴＄杽闂?JS/HTTP 閹稿洨姹楁稉鈧懛杈剧礉
// 閸氾箑鍨?Cloudflare閵嗕购eCAPTCHA 缁涘绱伴崶?UA 婢规壆袨 Chrome 娴ｅ棛宸辩亸?Sec-CH-UA / userAgentData 缁涘澹掑?
// 閸掋倕鐣炬稉鐑樻簚閸ｃ劋姹夐妴淇塱ndows 閻?WebView2閿涘湑hromium閿涘绱滾inux 閻?WebKitGTK閿涘苯鍨庨崚顐︹偓澶屾暏閸栧綊鍘ら惃?UA閵?
#[cfg(target_os = "macos")]
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.2 Safari/605.1.15";

#[cfg(all(unix, not(target_os = "macos")))]
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

const MENU_ID_PREV_CONVERSATION: &str = "polychat-prev-conversation";
const MENU_ID_NEXT_CONVERSATION: &str = "polychat-next-conversation";
const MENU_ID_SWITCH_PLATFORM_PREFIX: &str = "polychat-switch-platform-";
#[cfg(target_os = "windows")]
const HOTKEY_SWITCH_PLATFORM_BASE_ID: i32 = 0x5043_0100;
#[cfg(target_os = "windows")]
const HOTKEY_PREV_CONVERSATION_ID: i32 = 0x5043_0201;
#[cfg(target_os = "windows")]
const HOTKEY_NEXT_CONVERSATION_ID: i32 = 0x5043_0202;
#[cfg(target_os = "windows")]
const VK_OEM_4: u32 = 0xDB;
#[cfg(target_os = "windows")]
const VK_OEM_6: u32 = 0xDD;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewBounds {
  x: f64,
  y: f64,
  width: f64,
  height: f64,
  // React 鐟欏棗褰涢柅鏄忕帆妤傛ê瀹?window.innerHeight)閿涘瞼鏁ゆ禍搴㈠腹缁犳鐖ｆ０妯荤埉閸嬪繒些
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

fn frontend_platform_label(platform_id: &str) -> String {
  let sanitized: String = platform_id
    .chars()
    .map(|ch| {
      if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == ':' || ch == '/' {
        ch
      } else {
        '_'
      }
    })
    .collect();
  format!("platform-{sanitized}")
}

#[cfg(target_os = "macos")]
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

  // 鐎?WebView 閻ㄥ嫬娼楅弽鍥у斧閻愯妲哥粣妤€褰涢崘鍛啇閸栨椽銆婇柈顭掔礄閸氼偅鐖ｆ０妯荤埉娑撳閮ㄩ敍澶涚礉
  // 閼?React 閻?getBoundingClientRect 閸樼喓鍋ｉ弰?React 鐟欏棗褰涙い鍫曞劥閿涘牊鐖ｆ０妯荤埉娑斿绗呴敍澶堚偓?
  // 娴滃矁鈧懐娴夊顔荤娑擃亝鐖ｆ０妯荤埉妤傛ê瀹抽敍宀勬付鐟曚浇藟閸嬪灝鍩?y 娑撳绱濋崥锕€鍨€?WebView 閺佺繝缍嬫稉濠勑╅妴浣哥俺闁劎鏆€閻у鈧?
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
  #[cfg(target_os = "windows")]
  {
    return logical_rect_from_bounds(bounds);
  }

  #[cfg(not(target_os = "windows"))]
  Rect {
    position: tauri::Position::Physical(physical_position_from_bounds(window, &bounds)),
    size: tauri::Size::Physical(physical_size_from_bounds(window, &bounds)),
  }
}

#[cfg(target_os = "windows")]
fn logical_position_from_bounds(bounds: &ViewBounds) -> LogicalPosition<f64> {
  // Match Tauri's JS Webview API on Windows. WebView2 child coordinates are
  // logical pixels relative to the parent webview client area.
  LogicalPosition::new(bounds.x.round().max(0.0), bounds.y.round().max(0.0))
}

#[cfg(target_os = "windows")]
fn logical_size_from_bounds(bounds: &ViewBounds) -> LogicalSize<f64> {
  LogicalSize::new(bounds.width.round().max(1.0), bounds.height.round().max(1.0))
}

#[cfg(target_os = "windows")]
fn logical_rect_from_bounds(bounds: ViewBounds) -> Rect {
  Rect {
    position: tauri::Position::Logical(logical_position_from_bounds(&bounds)),
    size: tauri::Size::Logical(logical_size_from_bounds(&bounds)),
  }
}

fn has_visible_bounds(bounds: &ViewBounds) -> bool {
  bounds.width >= 16.0 && bounds.height >= 16.0
}

#[cfg(target_os = "windows")]
fn default_user_agent_override() -> Option<&'static str> {
  // WebView2 exposes Chromium client hints that must match the real runtime.
  // A hard-coded Edge UA can make login/challenge pages render blank on Windows.
  None
}

#[cfg(not(target_os = "windows"))]
fn default_user_agent_override() -> Option<&'static str> {
  Some(DEFAULT_USER_AGENT)
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
  // 閸樼粯甯€閺傚洣娆㈢化鑽ょ埠娑撳秴寮告總钘夌摟缁?
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
  // 缁犫偓閸栨牜娈?percent-decode閿涙稑銇戠拹銉ユ皑閻╁瓨甯存潻鏂挎礀閸樼喍瑕?
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

// 閸︺劍鐦℃稉顏呮煀 document 閸掓稑缂撻崥搴涒偓渚€銆夐棃?JS 閹笛嗩攽娑斿澧犻張鈧弮鈺傛暈閸忋儯鈧?
// 閻劋绨幎?WebView 閸?navigator 娑撳﹥鐣悾娆戞畱"閼奉亜濮╅崠鏍ㄥ瘹缁?娣囶喖娓鹃敍宀勪缉閸?Cloudflare/Turnstile 缁?
// 閸欏秶鍩囬張宥呭閸?challenge 闂冭埖顔岄惄瀛樺复閹跺﹥鍨滄禒顒€鍨界€规矮璐熼張鍝勬珤娴滄亽鈧?
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
  // navigator.webdriver: 閻喎鐤?Safari 濞屸剝婀佸銈呯潣閹?(undefined)閿涘矂鍎撮崚鍡氬殰閸斻劌瀵?WebView 娴兼碍姣氶棁韫礋 true
  try { delete Navigator.prototype.webdriver; } catch (_) {}
  safeDefine(navigator, 'webdriver', () => undefined);

  // navigator.languages: 閻喎鐤?Safari 闁艾鐖舵潻鏂挎礀 ['zh-CN','zh','en'] 娑斿琚惃鍕姜缁岀儤鏆熺紒?
  try {
    if (!navigator.languages || navigator.languages.length === 0) {
      const lang = navigator.language || 'en-US';
      const langs = lang.startsWith('zh') ? ['zh-CN', 'zh', 'en'] : [lang, 'en'];
      safeDefine(navigator, 'languages', () => langs);
    }
  } catch (_) {}

  // 闁劌鍨庡Λ鈧ù瀣壖閺堫兛绱扮拠?window.chrome閿涙瓗A 閺冦垻鍔ф竟鐗堟娑?Safari閿涘苯姘ㄦ惔鏃囶嚉濞屸剝婀?chrome 鐎电钖?
  try {
    if (/Safari/.test(navigator.userAgent) && !/Chrome|Chromium|Edg/.test(navigator.userAgent)) {
      try { delete window.chrome; } catch (_) {}
    }
  } catch (_) {}

  // Notification.permission 閸?WebView 娑擃厼褰查懗鍊熺箲閸?'denied'閿涘瞼婀＄€?Safari 姒涙顓?'default'
  try {
    if (typeof Notification !== 'undefined') {
      const original = Notification.permission;
      if (original === 'denied') {
        safeDefine(Notification, 'permission', () => 'default');
      }
    }
  } catch (_) {}

  // 閹?URL.createObjectURL閿涙氨绱︾€?Blob 娴犮儰绌堕崥搴ㄦ桨 a[download] 閹凤附鍩呮稉宥呯箑閸?fetch
  // 韫囧懘銆忛弨鎯ф躬 init script 閼板矂娼?page-load 濞夈劌鍙嗛敍灞芥儊閸掓瑩銆夐棃銏℃－閺堢喎鍨卞铏规畱 blob 閹峰じ绗夐崚鑸偓?
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
  // 娣囶喖顦?macOS WKWebView 閹锋帞绮?navigator.clipboard.write 閸愭瑥娴橀悧鍥╂畱闂勬劕鍩楅妴?
  // WKWebView 娑撳秴鍘戠拋?ClipboardItem(image/*)閿涘本瀚ら幋?clipboard.write閿涘本濡搁崶鍓у娴滃矁绻橀崚?
  // 闁俺绻?Tauri 閸涙垝鎶ゆ禍?arboard 閸愭瑥鍙嗙化鑽ょ埠閸擃亣鍒涢弶瑁も偓?
  // Windows/Linux 娑撳﹦娈?WebView2/WebKitGTK 閸樼喓鏁撻弨顖涘瘮閸ュ墽澧栭崜顏囧垱閺夊尅绱濋弮鐘绘付濞夈劌鍙?hook閵?
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
            }} catch (e) {{}}
          }}
          return orig(items);
        }};
        cb.__polyImageHook = true;
      }}
    }}
  }} catch (e) {{}}
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
    // 娴犲懏瀚ら幋?閻喐顒滅捄銊ョ厵"閻ㄥ嫭娅橀柅姘辩崶閸欙絾澧﹀鈧拠閿嬬湴閿涙稖顓荤拠浣歌剨缁愭ぞ绻氶悾娆忓斧閻?popup閿?
    // 闁灝鍘ら惍鏉戞綎 Google OAuth 鏉╂瑧琚笟婵婄 window.opener / postMessage 閻ㄥ嫭宸块弶鍐╃ウ缁嬪鈧?
    if (url && isExternalUrl(url) && !isAuthPopupUrl(url)) {{
      openInAppTab(url);
      return null;
    }}
    return originalOpen ? originalOpen.call(window, url, target, features) : null;
  }};

  // 閹?Blob/ArrayBuffer 鏉?base64閿涘苯鍨庨崸妞句簰闁灝鍘ゆ径褎鏋冩禒鎯扮Т鏉╁洩鐨熼悽銊︾垽闂勬劕鍩?
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
      reportDownloadFailure(filename, sourceUrl, (e && e.message) || 'toBase64 failed');
    }});
  }};

  // JS 閹凤附鍩呮径杈Е閺冩湹绗傞幎銉礉缂佹瑧鏁ら幋铚傜娑擃亜褰茬憴浣烘畱婢惰精瑙?toast
  const reportDownloadFailure = (filename, sourceUrl, reason) => {{
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

  // 閹凤附鍩?a[download]閿涙瓫.click() / 閻劍鍩涢悙鐟板毊鐢?download 鐏炵偞鈧呮畱闁剧偓甯撮妴?
  // WKWebView 姒涙顓绘稉宥勭窗婢跺嫮鎮婃潻娆戣娑撳娴囬敍灞界箑妞ゆ槒鍤滃杈嚢閸戝搫鍞寸€圭懓鍟撻惄妯糕偓?
  const tryInterceptDownloadAnchor = (anchor, event) => {{
    if (!anchor || !anchor.hasAttribute('download')) return false;
    const href = anchor.getAttribute('href') || anchor.href || '';
    if (!href) return false;
    const filename = anchor.getAttribute('download') || href.split(/[\\/?#]/).filter(Boolean).pop() || 'download';

    // 娴兼ê鍘涙担璺ㄦ暏 createObjectURL 缂傛挸鐡ㄩ柌宀€娈?blob閿涘矂浼╅崗宥呭晙 fetch閿涘牆绶㈡径姘辩彲閻愬湱娈?CSP 娑撳秴鍘戠拋?connect-src blob:閿?
    const cachedBlob = window.__POLYCHAT_BLOBS__?.get(href);
    if (cachedBlob) {{
      if (event) {{
        event.preventDefault();
        event.stopPropagation();
      }}
      sendDownload(cachedBlob, filename, href);
      return true;
    }}

    // blob: 娑?data: 閻╁瓨甯寸拠?
    if (href.startsWith('blob:') || href.startsWith('data:')) {{
      if (event) {{
        event.preventDefault();
        event.stopPropagation();
      }}
      fetch(href).then(r => r.blob()).then(b => sendDownload(b, filename, href)).catch((e) => {{
        reportDownloadFailure(filename, href, (e && e.message) || 'blob/data fetch failed');
      }});
      return true;
    }}
    // http(s) 鐠у嫭绨稊鐔稿复缁犫€茬瑓閺夈儻绱濋崥锕€鍨?WKWebView 婢舵艾宕愰惄瀛樺复閸?webview 闁插矁鐑︽潪顑锯偓?
    // 閸氬本绨敮?credentials閿涘牅绻氶悾娆戞瑜版洘鈧緤绱氶敍娑滄硶閸╃喎宸遍崚鏈电瑝鐢?credentials閿涘苯鎯侀崚娆庣窗鐟欙箑褰?CORS 婢惰精瑙﹂妴?
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
            // Fall back to the system browser when the in-webview fetch fails.
            openInSystemBrowser(url.href);
            reportDownloadFailure(filename, url.href, '已在系统浏览器中打开 (' + ((e && e.message) || 'fetch failed') + ')');
          }});
        return true;
      }}
    }} catch (_) {{}}
    return false;
  }};

  // 閸忔粌绨抽敍姘秼 DOM 闁插本鏌婃晶?<a download> 楠炴儼顫﹂崥鍫熷灇 click 閺冭绱欏鍫濐樋濡楀棙鐏﹂惃鍕瑓鏉炶棄鐤勯悳甯礆閿?
  // MutationObserver 閻绗夐崚?click 鐞涘奔璐熼敍灞肩稻閸欘垯浜掔憴鍌氱檪"閺傛澘顤冮懞鍌滃仯"閵嗗倻绮ㄩ崥鍫滅瑓闂堛垻娈?click 閻╂垵鎯夋径鐔活洬閻╂牕銇囨径姘殶閹懎鍠岄妴?
  // 娴ｅ棜瀚㈢純鎴犵彲鐠?window.location.href = blobUrl 鏉╂瑧顫掗弬鐟扮础閿涘矂娓剁憰浣稿礋閻欘剚瀚?location 鐠у鈧鈧?
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
            const filename = (v.split('#').pop() || 'download').replace(/[^A-Za-z0-9._\-]/g, '_');
            fetch(v).then(r => r.blob()).then(b => sendDownload(b, filename, v)).catch((e) => {{
              reportDownloadFailure(filename, v, (e && e.message) || 'location blob fetch failed');
            }});
            return;
          }}
          originalHrefSetter(value);
        }}
      }});
    }}
  }} catch (e) {{
  }}

  // createObjectURL 閹凤附鍩呭鍙夊皳閸?stealth_init_script閿涘牆鎯庨崝銊︽纯閺冣晪绱氶敍宀冪箹闁插奔绗夐崘宥夊櫢婢?hook閵?

  // 閸忔粌绨宠箛顐ｅ祹闁款噯绱癈md/Ctrl + Shift + S 娣囨繂鐡ㄨぐ鎾冲姒х姵鐖ｉ幃顒€浠犻惃鍕禈閻楀洤鍩?Downloads閵?
  // 閻劋绨純鎴犵彲娑撳娴囬幐澶愭尦娑撳秴浼愭担婊勬閻ㄥ嫪姹夊銉ュ幑鎼存洏鈧?
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
      fetch(src, {{ credentials: 'include', mode: 'cors' }})
        .then(r => r.blob())
        .then(b => sendDownload(b, filename, src))
        .catch((err) => {{
          // CORS 婢惰精瑙﹂弮璺虹毦鐠囨洑绮犲鎻掑鏉炵晫娈?<img> 閻㈣鍩?canvas 閸愬秷顕?
          try {{
            const canvas = document.createElement('canvas');
            canvas.width = img.naturalWidth || img.width;
            canvas.height = img.naturalHeight || img.height;
            canvas.getContext('2d').drawImage(img, 0, 0);
            canvas.toBlob((b) => b && sendDownload(b, (filename || 'image') + '.png', src), 'image/png');
          }} catch (e2) {{
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
  // 缁嬪绱＄拫鍐暏 a.click()閿涘牆绶㈡径?SPA 閻劏绻栫粔宥嗘煙瀵繗袝閸欐垳绗呮潪鏂ょ礆娑撳秳绱伴崘鎺撳満閸?document閿?
  // 閻╁瓨甯撮崷銊ュ斧閸ㄥ鐪伴棃銏″娑撳娼甸妴?
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
    const prev = makeButton('閳?, 'Previous');
    const next = makeButton('閳?, 'Next');

    const close = document.createElement('button');
    close.type = 'button';
    close.textContent = '鑴?;
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
  const EXCLUDED_CONVERSATION_TEXT_RE = /^(new chat|new conversation|start a new chat|new|history|\u65b0\u5bf9\u8bdd|\u65b0\u5efa\u5bf9\u8bdd|\u5386\u53f2|\u5bf9\u8bdd\u5386\u53f2)$/i;
  const DATE_GROUP_TEXT_RE = /^(today|yesterday|previous\s+\d+\s+days?|last\s+\d+\s+days?|\d{{4}}(?:[-/]\s*)?(?:0?[1-9]|1[0-2])|\d{{1,2}}\/\d{{1,2}}(?:\/\d{{2,4}})?)$/i;
  const ACTIVE_CLASS_RE = /(?:^|[\s_-])(active|selected|current|is-active|is-selected)(?:$|[\s_-])/;
  const normalizeConversationText = (text) => String(text || '').replace(/[\u200b-\u200f\u202a-\u202e]/g, '').replace(/\s+/g, ' ').trim();
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
        if (!/^(history|chat history|conversations?|\u5386\u53f2|\u5bf9\u8bdd\u5386\u53f2)$/i.test(text)) continue;
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

fn frontend_click_bridge_script(platform_id: &str, opener_view_id: &str) -> String {
  let platform_id = serde_json::to_string(platform_id).unwrap_or_else(|_| "\"\"".to_string());
  let opener_view_id = serde_json::to_string(opener_view_id).unwrap_or_else(|_| "\"\"".to_string());

  format!(
    r#"
(() => {{
  if (window.__POLYCHAT_FRONTEND_CLICK_BRIDGE__) return;
  window.__POLYCHAT_FRONTEND_CLICK_BRIDGE__ = true;
  const platformId = {platform_id};
  const openerViewId = {opener_view_id};

  const openInAppTab = (rawUrl) => {{
    if (!rawUrl) return;
    try {{
      const url = new URL(rawUrl, location.href).href;
      const promise = window.__TAURI_INTERNALS__?.invoke('open_platform_tab', {{
        platformId,
        openerViewId,
        url
      }});
      if (promise && typeof promise.catch === 'function') {{
        promise.catch(() => {{
          try {{ location.href = url; }} catch (_) {{}}
        }});
      }}
    }} catch (_) {{}}
  }};

  const findAnchor = (event) => {{
    const path = event.composedPath ? event.composedPath() : [];
    let anchor = path.find((item) => item && item.tagName === 'A');
    if (!anchor && event.target?.closest) anchor = event.target.closest('a[href]');
    return anchor;
  }};

  const shouldBridgeAnchor = (anchor, event) => {{
    if (!anchor || !anchor.href || anchor.hasAttribute('download')) return false;
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

    const target = String(anchor.target || '').toLowerCase();
    return target === '_blank' || parsed.origin !== location.origin || event.metaKey || event.ctrlKey || event.shiftKey;
  }};

  const originalOpen = window.open;
  window.open = function(url, target, features) {{
    if (url) {{
      try {{
        const parsed = new URL(url, location.href);
        if (/^https?:$/.test(parsed.protocol) && parsed.href !== location.href) {{
          openInAppTab(parsed.href);
          return null;
        }}
      }} catch (_) {{}}
    }}
    return originalOpen ? originalOpen.call(window, url, target, features) : null;
  }};

  document.addEventListener('click', (event) => {{
    if (event.defaultPrevented) return;
    const anchor = findAnchor(event);
    if (!shouldBridgeAnchor(anchor, event)) return;
    event.preventDefault();
    event.stopPropagation();
    openInAppTab(anchor.href);
  }}, true);

  document.addEventListener('auxclick', (event) => {{
    if (event.button !== 1 || event.defaultPrevented) return;
    const anchor = findAnchor(event);
    if (!shouldBridgeAnchor(anchor, event)) return;
    event.preventDefault();
    event.stopPropagation();
    openInAppTab(anchor.href);
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

  const EXCLUDED_CONVERSATION_TEXT_RE = /^(new chat|new conversation|start a new chat|new|history|\u65b0\u5bf9\u8bdd|\u65b0\u5efa\u5bf9\u8bdd|\u5386\u53f2|\u5bf9\u8bdd\u5386\u53f2)$/i;
  const DATE_GROUP_TEXT_RE = /^(today|yesterday|previous\s+\d+\s+days?|last\s+\d+\s+days?|\d{{4}}(?:[-/]\s*)?(?:0?[1-9]|1[0-2])|\d{{1,2}}\/\d{{1,2}}(?:\/\d{{2,4}})?)$/i;
  const ACTIVE_CLASS_RE = /(?:^|[\s_-])(active|selected|current|is-active|is-selected)(?:$|[\s_-])/;
  const normalizeConversationText = (text) => String(text || '').replace(/[\u200b-\u200f\u202a-\u202e]/g, '').replace(/\s+/g, ' ').trim();
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
        if (!/^(history|chat history|conversations?|\u5386\u53f2|\u5bf9\u8bdd\u5386\u53f2)$/i.test(text)) continue;
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
  if !has_visible_bounds(&bounds) {
    return Err(format!(
      "invalid initial webview bounds for {platform_id}: {},{} {}x{}",
      bounds.x, bounds.y, bounds.width, bounds.height
    ));
  }
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
  let blank_url = Url::parse("about:blank").map_err(|err| format!("invalid blank url: {err}"))?;
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
  // 閸?Requested 闂冭埖顔岀拋鏉跨秿閻╊喗鐖ｇ捄顖氱窞閿涘瓗inished 闂冭埖顔岄崶鐐诧綖閿涘潰acOS 娑?wry 娑撳秳绱伴崶鐐扮炊鐠侯垰绶為敍?
  let download_destinations: Arc<Mutex<HashMap<String, PathBuf>>> =
    Arc::new(Mutex::new(HashMap::new()));
  let download_dests_for_started = download_destinations.clone();
  let download_dests_for_finished = download_destinations.clone();
  let initial_title = platform_name.clone();
  let initial_url = url.clone();

  #[allow(unused_mut)]
  let mut builder = WebviewBuilder::new(platform_label, WebviewUrl::External(blank_url))
    .devtools(cfg!(debug_assertions))
    .initialization_script(stealth_init_script());

  // Windows WebView2 can hang while creating child webviews with a per-view
  // user data folder. Use the default WebView2 profile there so add_child
  // returns reliably; keep isolated data dirs on platforms where this path is stable.
  #[cfg(not(target_os = "windows"))]
  {
    builder = builder.data_directory(data_dir);
  }

  let effective_user_agent = user_agent.or_else(|| default_user_agent_override().map(str::to_string));
  if let Some(user_agent) = effective_user_agent.as_deref() {
    builder = builder.user_agent(user_agent);
  }

  #[cfg(target_os = "macos")]
  {
    builder = builder.data_store_identifier(data_store_identifier(&storage_id));
  }

  let builder = builder
    .on_download(move |_webview, event| match event {
      DownloadEvent::Requested { url, destination } => {
        // 閹跺﹦娲伴弽鍥熅瀵板嫬鐣鹃崚鎵兇缂?~/Downloads/<閺傚洣娆㈤崥?閿涘苯鎮撻崥宥堟嫹閸?-1/-2閳?
        let dir = app_for_download
          .path()
          .download_dir()
          .or_else(|_| app_for_download.path().home_dir().map(|h| h.join("Downloads")))
          .unwrap_or_else(|_| PathBuf::from("."));
        let filename = preferred_download_filename(&url, destination);
        let target = unique_path(dir.join(filename));
        if let Ok(mut map) = download_dests_for_started.lock() {
          map.insert(url.to_string(), target.clone());
        }
        *destination = target;
        true
      }
      DownloadEvent::Finished { url, path, success } => {
        // macOS 娑?wry 閻?path 娑撯偓閻╃繝璐?None閿涘瞼鏁?Requested 閺冨墎绱︾€涙娈戦惄顔界垼鐠侯垰绶為崶鐐诧綖
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
      // 娴犲懎顕捄銊ョ厵 http(s) 瀵湱鐛ユ潪顑胯礋閺傜増鐖ｇ粵鎾呯幢閸氬本绨鍦崶閿涘湦Auth/CF 閹告垶鍨粵澶涚礆娣囨繃瀵旈崢鐔烘晸鐞涘奔璐熼敍?
      // 闁灝鍘ら惍鏉戞綎娓氭繆绂?window 瀵洜鏁ら惃鍕礀鐠嬪啴鈧矮淇婇妴?
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
    .initialization_script(&tab_interceptor_script(&storage_id, &platform_id))
    .initialization_script(&format!(
      "window.__POLYCHAT_TITLE_POLL__&&clearInterval(window.__POLYCHAT_TITLE_POLL__);window.__POLYCHAT_TITLE_POLL__=setInterval(()=>{{window.__TAURI_INTERNALS__?.invoke('update_platform_title',{{platformId:'{pid}',title:document.title||'',url:location.href||'',canGoBack:history.length>1,canGoForward:false}})}},1000);window.__TAURI_INTERNALS__?.invoke('update_platform_title',{{platformId:'{pid}',title:document.title||'',url:location.href||'',canGoBack:history.length>1,canGoForward:false}});",
      pid = platform_id
    ))
    .on_page_load(move |_webview, payload| {
      let app_handle = app_for_load.clone();
      let platform_id = platform_for_load.clone();
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

  #[cfg(target_os = "windows")]
  let webview = window
    .add_child(
      builder,
      logical_position_from_bounds(&bounds),
      logical_size_from_bounds(&bounds),
    )
    .map_err(|err| format!("failed to create webview: {err}"))?;

  #[cfg(not(target_os = "windows"))]
  let webview = window
    .add_child(
      builder,
      physical_position_from_bounds(&window, &bounds),
      physical_size_from_bounds(&window, &bounds),
    )
    .map_err(|err| format!("failed to create webview: {err}"))?;
  webview
    .show()
    .map_err(|err| format!("failed to show initial webview: {err}"))?;

  let state = PlatformViewState {
    title: initial_title,
    can_go_back: false,
    can_go_forward: false,
    loading: true,
    url: initial_url,
  };

  emit_platform_state(&app, &platform_id, &state);
  let mut view_map = views.0.lock().map_err(|_| "platform view lock poisoned")?;
  view_map.insert(platform_id.clone(), PlatformView { webview, state });
  let webview = view_map
    .get(&platform_id)
    .map(|view| view.webview.clone())
    .ok_or_else(|| format!("platform view not found after create: {platform_id}"))?;
  drop(view_map);

  webview
    .navigate(parsed_url)
    .map_err(|err| format!("failed to navigate initial webview: {err}"))?;

  let view_map = views.0.lock().map_err(|_| "platform view lock poisoned")?;
  Ok(get_state_payload(&platform_id, view_map.get(&platform_id).unwrap()))
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
      // 閹跺﹦鍔嶉悙瑙勬▔瀵繋姘︾紒娆愭煀閺勫墽銇氶惃?WebView閿涘矂浼╅崗宥嗗瘻闁款喕绨ㄦ禒鍓佹埛缂侇厽濮囬柅鎺戝煂閸掓俺顫﹂梾鎰閻?WebView閿?
      // 閸氾箑鍨崷?macOS 娑撳绻涚紒顓⌒曢崣鎴濇彥閹圭兘鏁弮鏈电窗"娑撱垽鏁?閵?
      let _ = webview.set_focus();
    } else {
      webview.hide().map_err(|err| err.to_string())?;
    }
  }
  Ok(())
}

/// 閸掑棗鐫嗗Ο鈥崇础閿涙艾鎮撻弮鑸垫▔缁€?platform_ids 娑擃厾娈戦幍鈧張?WebView閿涘矂娈ｉ挊蹇撳従娴ｆ瑣鈧?
/// 閸欘亝濡搁悞锔惧仯娴溿倗绮伴梿鍡楁値娑擃厾娈戠粭顑跨娑擃亷绱檖rimary閿涘绱濋柆鍨帳婢舵矮閲?WebView 娴滄帞娴夐幎銏㈠妽閻愬箍鈧?
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
    if !has_visible_bounds(&bounds) {
      return Ok(());
    }
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
fn install_platform_view_hooks(
  app: AppHandle,
  view_id: String,
  platform_id: String,
) -> Result<(), String> {
  let label = frontend_platform_label(&view_id);
  let webview = app
    .get_webview(&label)
    .ok_or_else(|| format!("platform view not found: {view_id}"))?;

  webview
    .eval(frontend_click_bridge_script(&platform_id, &view_id))
    .map_err(|err| err.to_string())?;
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
  app: AppHandle,
  views: tauri::State<'_, PlatformViews>,
  platform_id: String,
  offset: i32,
) -> Result<(), String> {
  let managed_webview = {
    let views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
    views.get(&platform_id).map(|view| view.webview.clone())
  };

  let webview = if let Some(webview) = managed_webview {
    webview
  } else {
    let label = frontend_platform_label(&platform_id);
    app.get_webview(&label)
      .ok_or_else(|| format!("platform view not found: {platform_id}"))?
  };

  webview
    .eval(conversation_switch_script(offset))
    .map_err(|err| err.to_string())?;
  Ok(())
}

/// 閻㈢喐鍨氶妴灞惧Ω閺傚洦婀版繅顐㈠帠鏉╂稓缍夋い浣冪翻閸忋儲顢嬮妴宥囨畱濞夈劌鍙嗛懘姘拱閵?
/// `json_text` 韫囧懘銆忛弰顖氬嚒缂佸繗绻?serde_json 缂傛牜鐖滈惃鍕暔閸?JS 鐎涙顑佹稉鎻掔摟闂堛垽鍣洪敍鍫濇儓娑撱倗顏鏇炲娇閿涘鈧?
/// 閹?brand 闁瀚ㄩ崐娆撯偓澶愨偓澶嬪閸ｎ煉绱濇繅顐㈠帠閺傜懓绱￠弽瑙勫祦閸忓啰绀岀猾璇茬€烽懛顏堚偓鍌氱安閿?
///   - textarea / input閿涙氨鏁ら崢鐔风€?value setter + dispatch input 娴滃娆㈤敍鍫濆悑鐎?React 閸欐甯剁紒鍕閿?
///   - contenteditable閿涙瓲ocus + 閸忋劑鈧?+ execCommand('insertText')
/// 娴犲懎锝為崗鍜冪礉娑撳秷袝閸欐垵褰傞柅浣碘偓鍌涘娑撳秴鍩岄崗鍐閺冩儼鐨熼悽?閿涘矂娼ゆ妯圭瑝閹舵盯鏁婇妴?
fn fill_input_script(brand: &str, json_text: &str) -> String {
  // 濮ｅ繋閲?brand 閻ㄥ嫬鈧瑩鈧鈧瀚ㄩ崳顭掔礄best-effort閿涘奔绮犵划鍓р€橀崚鏉款啍閺夋儳娲栭柅鈧敍?
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
      "[contenteditable=\"true\"][data-placeholder]",
      "div[contenteditable=\"true\"][role=\"textbox\"]",
      "div.ProseMirror[contenteditable=\"true\"]",
      "[contenteditable=\"true\"]",
      "textarea#chat-input",
      "textarea[placeholder]",
      "textarea",
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
  function sendEnter(el) {{
    function fire(type) {{
      el.dispatchEvent(new KeyboardEvent(type, {{
        key: 'Enter', code: 'Enter', keyCode: 13, which: 13,
        bubbles: true, cancelable: true, composed: true
      }}));
    }}
    fire('keydown');
    fire('keypress');
    fire('keyup');
  }}
  function fireInput(el, inputType, data) {{
    try {{
      el.dispatchEvent(new InputEvent('input', {{
        inputType: inputType || 'insertText',
        data: data || null,
        bubbles: true,
        cancelable: false,
        composed: true
      }}));
    }} catch (_) {{
      el.dispatchEvent(new Event('input', {{ bubbles: true, cancelable: false }}));
    }}
  }}
  function fireBeforeInput(el, inputType, data) {{
    try {{
      return el.dispatchEvent(new InputEvent('beforeinput', {{
        inputType: inputType || 'insertText',
        data: data || null,
        bubbles: true,
        cancelable: true,
        composed: true
      }}));
    }} catch (_) {{
      return true;
    }}
  }}
  function placeCaretAtEnd(el) {{
    try {{
      var range = document.createRange();
      range.selectNodeContents(el);
      range.collapse(false);
      var sel = window.getSelection();
      sel.removeAllRanges();
      sel.addRange(range);
    }} catch (_) {{}}
  }}
  try {{
    var el = pick();
    if (!el) {{ return; }}
    var tag = (el.tagName || '').toLowerCase();
    var filled = false;
    if (tag === 'textarea' || tag === 'input') {{
      var proto = tag === 'textarea'
        ? window.HTMLTextAreaElement.prototype
        : window.HTMLInputElement.prototype;
      var setter = Object.getOwnPropertyDescriptor(proto, 'value').set;
      el.focus();
      setter.call(el, text);
      fireInput(el, 'insertText', text);
      el.dispatchEvent(new Event('change', {{ bubbles: true }}));
      filled = true;
    }} else if (el.isContentEditable) {{
      el.focus();
      var sel = window.getSelection();
      var range = document.createRange();
      range.selectNodeContents(el);
      sel.removeAllRanges();
      sel.addRange(range);
      fireBeforeInput(el, 'insertText', text);
      var ok = document.execCommand('insertText', false, text);
      if (!ok) {{
        el.textContent = text;
        placeCaretAtEnd(el);
      }}
      fireInput(el, 'insertText', text);
      el.dispatchEvent(new Event('change', {{ bubbles: true }}));
      el.dispatchEvent(new KeyboardEvent('keyup', {{ bubbles: true, cancelable: true, composed: true }}));
      filled = true;
    }} else {{}}
    if (filled) {{
      // 瀵よ埖妞傞崘宥呭絺闁緤绱濈紒?React 閸欐甯剁紒鍕閻?value 閺囧瓨鏌婇悾娆忓毉閺冨爼妫?
      setTimeout(function() {{
        try {{ sendEnter(el); }} catch (e) {{}}
      }}, 120);
    }}
  }} catch (e) {{}}
}})();"#
  )
}

/// 楠炴寧鎸辨繅顐㈠帠閿涙碍濡?text 婵夘偄鍙嗛幐鍥х暰楠炲啿褰?WebView 閻ㄥ嫯绶崗銉︻攱閿涘牅绗夌憴锕€褰傞崣鎴︹偓渚婄礆閵?
/// brand 閻劋绨崚鍡樻烦閸氬嫬閽╅崣棰佺瑝閸氬瞼娈?DOM 闁瀚ㄩ崳顭掔礄閸撳秶顏弰鎯х础娴肩姴鍙嗛敍宀冩硶 clone/闁插秴鐣鹃崥鎴犌旂€规熬绱氶妴?
#[tauri::command]
fn fill_platform_input(
  app: AppHandle,
  views: tauri::State<'_, PlatformViews>,
  platform_id: String,
  brand: String,
  text: String,
) -> Result<(), String> {
  let managed_webview = {
    let views = views.0.lock().map_err(|_| "platform view lock poisoned")?;
    views.get(&platform_id).map(|view| view.webview.clone())
  };

  let webview = if let Some(webview) = managed_webview {
    webview
  } else {
    let label = frontend_platform_label(&platform_id);
    app.get_webview(&label)
      .ok_or_else(|| format!("platform view not found: {platform_id}"))?
  };

  // 閺傚洦婀扮紒?serde_json 缂傛牜鐖滄稉鍝勭暔閸?JS 鐎涙顑佹稉鎻掔摟闂堛垽鍣洪敍灞炬建缂?JS 濞夈劌鍙?
  let json_text = serde_json::to_string(&text).map_err(|err| err.to_string())?;
  webview
    .eval(fill_input_script(&brand, &json_text))
    .map_err(|err| err.to_string())?;
  Ok(())
}

/// 濞夈劌鍙嗛懘姘拱闁插瞼娈戠拠濠冩焽 sink閿涙艾缍?clipboard hook 閹存牕鍙剧€瑰啯鏁為崗銉┾偓鏄忕帆閸涙垝鑵戦柨娆掝嚖閸掑棙鏁弮璁圭礉
/// 閹跺﹣绗傛稉瀣瀮閹垫挸鍩?stderr 閺傞€涚┒閹烘帗鐓￠敍鍫濆涧閺堝銇戠拹銉ㄧ熅瀵板嫯鐨熼悽顭掔礉濮濓絽鐖舵担璺ㄦ暏闂嗚泛绱戦柨鈧敍澶堚偓?
/// 閹?base64 缂傛牜鐖滈惃鍕禈閻楀浄绱橮NG/JPEG/GIF 缁涘鎹㈤幇?image crate 閺€顖涘瘮閻ㄥ嫭鐗稿蹇ョ礆閸愭瑥鍙嗙化鑽ょ埠閸擃亣鍒涢弶瑁も偓?
/// 鐟欙絽鍠?macOS WKWebView 閹锋帞绮?`navigator.clipboard.write(ClipboardItem)` 閸愭瑥娴橀悧鍥╂畱闂勬劕鍩楅敍?
/// 濞夈劌鍙嗛懘姘拱閹凤附鍩呯純鎴︺€?clipboard.write閿涘本濡搁崶鍓у娴滃矁绻橀崚?base64 閸氬簼绱剁紒娆愭拱閸涙垝鎶ら敍宀€鏁遍崢鐔烘晸 arboard 閸愭瑥鍙嗛妴?
#[tauri::command]
fn copy_image_to_clipboard(data_base64: String) -> Result<(), String> {
  use base64_decode_compat as decode;
  let bytes = decode(&data_base64).map_err(|e| format!("invalid base64: {e}"))?;
  // arboard 鐟曚焦鐪?RGBA8 閸樼喎顫愰崓蹇曠閿涘本澧嶆禒銉ュ帥閻?image crate 鐟欙絿鐖滈崘宥呮澓鏉╂稑骞?
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
  Ok(())
}

/// 閹恒儲鏁归弶銉ㄥ殰濞夈劌鍙嗛懘姘拱閻?blob/data 娑撳娴囩拠閿嬬湴閵?
/// JS 缁旑垱瀚ら幋顏冪啊 a[download] 閻ㄥ嫮鍋ｉ崙姹団偓浣瑰Ω閸愬懎顔愮拠缁樺灇 base64 娴肩姾绻冮弶銉幢
/// 閹存垳婊戠憴锝囩垳閸氬骸鍟撻崗銉ч兇缂?Downloads 閻╊喖缍嶉敍灞藉晙閸欐垳绗岄崢鐔烘晸娑撳娴囬崥灞借埌閹胶娈戠€瑰本鍨氭禍瀣╂閵?
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

/// JS 缁旑垱瀚ら幋顏勫煂娑撳娴囬妴浣风稻鐠囪褰?鏉烆剛鐖滄径杈Е閺冩儼鐨熼悽顭掔礉閸氭垵澧犵粩顖氬絺閸氬苯鑸伴幀浣烘畱婢惰精瑙︽禍瀣╂閵?
/// 娣囨繆鐦夐悽銊﹀煕閼峰啿鐨惇瀣煂娑撯偓濞?toast閿涘奔绗夋导?閻愰€涚啊濞屸€冲冀鎼?閵?
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

/// 閺嬩胶鐣?base64 鐟欙絿鐖滈敍鍫滅瑝瀵洖鍙嗘０婵嗩樆娓氭繆绂嗛敍澶堚偓?
fn base64_decode_compat(s: &str) -> Result<Vec<u8>, String> {
  // 鐎圭懓绻婇幑銏ｎ攽/缁岃櫣娅?
  let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
  // 閸樼粯甯€ data URL 閸撳秶绱戦敍灞肩伐婵?data:application/pdf;base64,xxxx
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
  let mut switch_platform_items = Vec::new();
  for index in 1..=9 {
    switch_platform_items.push(MenuItem::with_id(
      app,
      format!("{MENU_ID_SWITCH_PLATFORM_PREFIX}{index}"),
      format!("Switch Platform {index}"),
      true,
      Some(format!("CmdOrCtrl+{index}")),
    )?);
  }
  let polychat_menu = Submenu::with_id_and_items(
    app,
    "polychat-actions",
    "PolyChat",
    true,
    &[
      &switch_platform_items[0],
      &switch_platform_items[1],
      &switch_platform_items[2],
      &switch_platform_items[3],
      &switch_platform_items[4],
      &switch_platform_items[5],
      &switch_platform_items[6],
      &switch_platform_items[7],
      &switch_platform_items[8],
      &prev_conversation,
      &next_conversation,
    ],
  )?;
  menu.append(&polychat_menu)?;
  Ok(menu)
}

#[cfg(target_os = "windows")]
fn register_windows_platform_hotkeys(app: AppHandle, window: Window) {
  let Ok(hwnd) = window.hwnd() else {
    return;
  };
  let main_hwnd = hwnd.0 as isize;

  thread::spawn(move || unsafe {
    for index in 1..=9 {
      let id = HOTKEY_SWITCH_PLATFORM_BASE_ID + index as i32;
      let vk = b'0' as u32 + index as u32;
      let _ = RegisterHotKey(std::ptr::null_mut(), id, MOD_CONTROL, vk);
    }
    let _ = RegisterHotKey(
      std::ptr::null_mut(),
      HOTKEY_PREV_CONVERSATION_ID,
      MOD_CONTROL | MOD_SHIFT,
      VK_OEM_4,
    );
    let _ = RegisterHotKey(
      std::ptr::null_mut(),
      HOTKEY_NEXT_CONVERSATION_ID,
      MOD_CONTROL | MOD_SHIFT,
      VK_OEM_6,
    );

    let mut msg: MSG = std::mem::zeroed();
    while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
      if msg.message != WM_HOTKEY {
        continue;
      }

      let id = msg.wParam as i32;

      if GetForegroundWindow() as isize != main_hwnd {
        continue;
      }

      if id == HOTKEY_PREV_CONVERSATION_ID || id == HOTKEY_NEXT_CONVERSATION_ID {
        let _ = app.emit(
          "polychat-shortcut",
          ShortcutEvent {
            action: "switch-conversation".to_string(),
            index: None,
            offset: Some(if id == HOTKEY_NEXT_CONVERSATION_ID { 1 } else { -1 }),
          },
        );
        continue;
      }

      let index = id - HOTKEY_SWITCH_PLATFORM_BASE_ID;
      if (1..=9).contains(&index) {
        let _ = app.emit(
          "polychat-shortcut",
          ShortcutEvent {
            action: "switch-platform".to_string(),
            index: Some(index as u32),
            offset: None,
          },
        );
      }
    }
  });
}

#[cfg(not(target_os = "windows"))]
fn register_windows_platform_hotkeys(_app: AppHandle, _window: Window) {}

pub fn run() {
  tauri::Builder::default()
    .manage(PlatformViews::default())
    .setup(|app| {
      app.set_menu(build_app_menu(app)?)?;
      if let Some(window) = app.get_window("main") {
        register_windows_platform_hotkeys(app.handle().clone(), window);
      }
      Ok(())
    })
    .on_menu_event(|app, event| {
      let id = event.id().0.as_str();
      if id == "quit" {
        app.exit(0);
      } else if id == MENU_ID_PREV_CONVERSATION {
        let _ = app.emit(
          "polychat-shortcut",
          ShortcutEvent {
            action: "switch-conversation".to_string(),
            index: None,
            offset: Some(-1),
          },
        );
      } else if id == MENU_ID_NEXT_CONVERSATION {
        let _ = app.emit(
          "polychat-shortcut",
          ShortcutEvent {
            action: "switch-conversation".to_string(),
            index: None,
            offset: Some(1),
          },
        );
      } else if let Some(raw_index) = id.strip_prefix(MENU_ID_SWITCH_PLATFORM_PREFIX) {
        if let Ok(index) = raw_index.parse::<u32>() {
          let _ = app.emit(
            "polychat-shortcut",
            ShortcutEvent {
              action: "switch-platform".to_string(),
              index: Some(index),
              offset: None,
            },
          );
        }
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
      install_platform_view_hooks,
      quit_app,
      dispatch_shortcut,
      switch_conversation,
      copy_image_to_clipboard,
      save_download_blob,
      report_download_error
    ])
    .on_window_event(|window, event| {
      if let tauri::WindowEvent::CloseRequested { .. } = event {
        if let Some(views) = window.app_handle().try_state::<PlatformViews>() {
          if let Ok(mut views) = views.0.lock() {
            for (_id, view) in views.drain() {
              let _ = view.webview.close();
            }
          }
        }
        window.app_handle().exit(0);
      }
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
