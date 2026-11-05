import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import About, { aboutInfo } from '../src/About'

const current = { schemaVersion: 1, version: '0.1.5', automaticUpdates: false, updateAvailable: false, updateMessage: '自动更新功能尚未接通，当前不会下载或安装更新。' }

describe('local about page', () => {
  it('accepts actual server version and honest unavailable update status', () => {
    expect(aboutInfo(current)).toEqual(current)
    expect(aboutInfo({ ...current, version: '0.2.0-beta.1' }).version).toBe('0.2.0-beta.1')
    for (const value of [{ ...current, version: '' }, { ...current, version: '0.1.5<script>' }, { ...current, automaticUpdates: 'false' }, { ...current, updateMessage: '' }, { ...current, secret: 'unexpected' }]) {
      expect(() => aboutInfo(value)).toThrow()
    }
  })
  it('offers update action without inventing a version or successful update before loading', () => {
    const html = renderToStaticMarkup(createElement(About))
    expect(html).toContain('关于未来教室')
    expect(html).toContain('正在读取版本')
    expect(html).toContain('立即更新')
    expect(html).not.toContain('0.1.5')
    expect(html).not.toContain('已是最新版本')
  })
})
