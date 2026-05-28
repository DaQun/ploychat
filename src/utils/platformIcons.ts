export const KNOWN_FAVICON_URLS: Record<string, string[]> = {
  'doubao.com': [
    'https://lf-flow-web-cdn.doubao.com/obj/flow-doubao/favicon/64x64.png',
    'https://lf-flow-web-cdn.doubao.com/obj/flow-doubao/doubao/chat/favicon.png',
  ],
  'chat.qwen.ai': [
    'https://assets.alicdn.com/g/qwenweb/qwen-chat-fe/0.2.57/favicon.png',
  ],
  'chat.deepseek.com': [
    'https://cdn.deepseek.com/chat/icon.png',
    'https://www.deepseek.com/favicon.ico',
  ],
  'chatgpt.com': [
    'https://cdn.oaistatic.com/assets/favicon-miwirzcw.ico',
  ],
  'claude.ai': [
    'https://cdn.prod.website-files.com/6889473510b50328dbb70ae6/689f4a9aff1f63fde75cf733_favicon.png',
  ],
}

export const normalizeIconDomain = (domain: string) =>
  domain.replace(/^https?:\/\//, '').replace(/\/.*$/, '')

export const getFaviconCandidates = (domain: string) => {
  const normalizedDomain = normalizeIconDomain(domain)
  return [
    ...(KNOWN_FAVICON_URLS[normalizedDomain] ?? []),
    `https://${normalizedDomain}/favicon.ico`,
    `https://www.google.com/s2/favicons?domain=${normalizedDomain}&sz=64`,
  ]
}

export const getPrimaryFaviconUrl = (domain: string) => getFaviconCandidates(domain)[0]
