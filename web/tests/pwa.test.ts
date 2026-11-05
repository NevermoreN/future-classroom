import { afterEach, describe, expect, it, vi } from 'vitest'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { runInNewContext } from 'node:vm'
import { serviceWorkerSource } from '../scripts/pwa-build'
import { installGuidance } from '../src/pwa-install'
import PwaInstallPrompt from '../src/PwaInstallPrompt'

describe('PWA installation guidance', () => {
  afterEach(() => vi.unstubAllGlobals())
  function renderPrompt(userAgent: string, installed = false, dismissed = false) {
    vi.stubGlobal('window', { matchMedia: () => ({ matches: installed }), isSecureContext: true })
    vi.stubGlobal('navigator', { userAgent, platform: 'iPhone', maxTouchPoints: 1, standalone: installed })
    vi.stubGlobal('sessionStorage', { getItem: () => dismissed ? 'yes' : null })
    return renderToStaticMarkup(createElement(PwaInstallPrompt))
  }
  it('shows iPhone sharing instructions immediately without a browser install event or extra click', () => {
    const html = renderPrompt('iPhone Version/18.0 Mobile Safari/604.1')
    expect(html).toContain('添加到桌面提示')
    expect(html).toContain('分享')
    expect(html).toContain('添加到主屏幕')
    expect(html).toContain('编辑操作')
    expect(html).not.toContain('>安装到桌面<')
  })
  it('directs embedded iPhone browsers to Safari and retains help after dismissal', () => {
    expect(renderPrompt('iPhone MicroMessenger')).toContain('系统浏览器')
    expect(renderPrompt('iPhone', false, true)).toContain('安装帮助')
    expect(renderPrompt('iPhone', false, true)).not.toContain('添加到桌面提示')
    expect(renderPrompt('iPhone', true)).toBe('')
  })
  it('offers actual Safari steps on iPhone and touch iPad, and external-browser guidance inside chat apps', () => {
    const environment = { userAgent: 'iPhone', platform: 'iPhone', touchPoints: 1, standalone: false }
    expect(installGuidance(environment).steps).toContain('分享')
    expect(installGuidance({ ...environment, userAgent: 'Macintosh', platform: 'MacIntel', touchPoints: 5 }).mobile).toBe(true)
    expect(installGuidance({ ...environment, userAgent: 'iPhone MicroMessenger' }).steps).toContain('系统浏览器')
    expect(installGuidance({ ...environment, userAgent: 'Android Chrome' }).steps).toContain('浏览器菜单')
    expect(installGuidance({ ...environment, userAgent: 'Windows Chrome', touchPoints: 0 }).mobile).toBe(false)
  })
})

function worker(base = '/fc/') {
  const handlers: Record<string, (event: any) => void> = {}
  const stored: { url: string; options: RequestInit }[] = []
  const deleted: string[] = []
  const cache = { addAll: async (requests: { url: string; options: RequestInit }[]) => { stored.push(...requests) }, match: async (path: string) => new Response(`cached:${path}`) }
  const self = { location: { origin: 'https://192.168.1.2:8443' }, addEventListener: (event: string, handler: (event: any) => void) => { handlers[event] = handler } }
  runInNewContext(serviceWorkerSource(base, ['assets/app-abc.js', 'index.html', 'offline.html', 'manifest.webmanifest'], 'v1'), {
    self, URL, Request: class { constructor(public url: string, public options: RequestInit) {} },
    caches: { open: async () => cache, keys: async () => [`fc-static:${base}:old`, 'some-other-app', 'fc-static:/other/:old'], delete: async (name: string) => { deleted.push(name) } },
    fetch: async () => { throw new Error('network offline') },
  })
  return { handlers, stored, deleted, origin: self.location.origin }
}

describe('PWA cache boundary', () => {
  it('preloads only public build assets with credentials omitted and leaves sensitive requests untouched', async () => {
    const owner = worker(); let installed: Promise<void> | undefined
    owner.handlers.install!({ waitUntil: (promise: Promise<void>) => { installed = promise } }); await installed
    expect(owner.stored.map(item => item.url)).toEqual(['/fc/assets/app-abc.js', '/fc/index.html', '/fc/offline.html', '/fc/manifest.webmanifest'])
    expect(owner.stored.every(item => item.options.credentials === 'omit')).toBe(true)
    for (const [path, method, mode] of [['/fc/api/v1/session', 'GET', 'cors'], ['/fc/api/v1/voice', 'POST', 'cors'], ['/fc/', 'POST', 'navigate'], ['/fc/?token=secret', 'GET', 'navigate'], ['/fc/repair', 'GET', 'navigate'], ['/fc/assets/app-abc.js?credential=x', 'GET', 'cors'], ['https://other.test/fc/assets/app-abc.js', 'GET', 'cors']]) {
      let intercepted = false
      owner.handlers.fetch!({ request: { url: new URL(path!, owner.origin).href, method, mode }, respondWith: () => { intercepted = true } })
      expect(intercepted, path).toBe(false)
    }
  })
  it('opens the cached app shell offline only on exact application navigation and isolates caches per base', async () => {
    const owner = worker('/pwa/'); let response: Promise<Response> | undefined
    owner.handlers.fetch!({ request: { url: `${owner.origin}/pwa/`, method: 'GET', mode: 'navigate' }, respondWith: (promise: Promise<Response>) => { response = promise } })
    expect(await (await response)!.text()).toBe('cached:/pwa/index.html')
    let activation: Promise<void> | undefined
    owner.handlers.activate!({ waitUntil: (promise: Promise<void>) => { activation = promise } }); await activation
    expect(owner.deleted).toEqual(['fc-static:/pwa/:old'])
  })
})
