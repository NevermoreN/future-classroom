import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { connectionInfo, ConnectionDetails } from '../src/QuickConnect'

const ready = { schemaVersion: 1, schoolName: '学校', className: '三年级二班', entryUrl: 'https://192.168.10.212:8443/fc/', unavailableReason: null }
describe('public quick connection', () => {
  it('renders a local SVG QR with visible complete URL and trust instructions', () => {
    const info = connectionInfo(ready), html = renderToStaticMarkup(createElement(ConnectionDetails, { info }))
    expect(html).toContain('<svg')
    expect(html).toContain('href="https://192.168.10.212:8443/fc/#/rooms"')
    expect(html).not.toContain('href="https://192.168.10.212:8443/fc/"')
    expect(html).toContain('三年级二班')
    expect(html).toContain('复制地址')
    expect(html).toContain('学校根证书')
    expect(html).not.toContain('<img')
  })
  it('does not render a QR, website link or copy action while unavailable', () => {
    const info = connectionInfo({ ...ready, entryUrl: null, unavailableReason: '教师HTTPS尚未启用。' })
    const html = renderToStaticMarkup(createElement(ConnectionDetails, { info }))
    expect(html).not.toContain('<svg')
    expect(html).not.toContain('复制地址')
    expect(html).toContain('教师HTTPS尚未启用。')
  })
  it('rejects management addresses, insecure URLs and embedded credentials', () => {
    for (const entryUrl of ['https://127.0.0.1:18443/fc/', 'https://localhost/fc/', 'http://192.168.10.212:8443/fc/', 'https://user:secret@192.168.10.212/fc/', 'https://192.168.10.212/fc/?token=secret', 'https://[::1]/fc/', 'https://0.0.0.0/fc/']) {
      expect(() => connectionInfo({ ...ready, entryUrl })).toThrow()
    }
    expect(() => connectionInfo({ ...ready, entryUrl: null })).toThrow()
  })
})
