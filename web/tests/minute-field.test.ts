import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import MinuteField, { editTimeDigits, minuteText } from '../src/MinuteField'

describe('fixed minute input', () => {
  it('formats minutes including the existing end-of-day boundary', () => {
    expect([0, 9, 60, 510, 1439, 1440].map(minuteText)).toEqual(['00:00', '00:09', '01:00', '08:30', '23:59', '24:00'])
  })

  it('overwrites digits and crosses the fixed colon without changing length', () => {
    expect(editTimeDigits('08:30', 0, 5, '1245')).toEqual({ text: '12:45', caret: 5 })
    expect(editTimeDigits('08:30', 1, 1, '9')).toEqual({ text: '09:30', caret: 3 })
    expect(editTimeDigits('08:30', 2, 2, '45')).toEqual({ text: '08:45', caret: 5 })
    expect(editTimeDigits('08:30', 5, 5, '9')).toEqual({ text: '08:30', caret: 5 })
  })

  it('clears selected digits or the neighbouring digit while preserving the colon', () => {
    expect(editTimeDigits('18:35', 1, 4, 'Delete').text).toBe('10:05')
    expect(editTimeDigits('18:35', 3, 3, 'Backspace')).toEqual({ text: '10:35', caret: 1 })
    expect(editTimeDigits('18:35', 2, 2, 'Delete')).toEqual({ text: '18:05', caret: 2 })
    expect(editTimeDigits('18:35', 0, 5, 'Backspace').text).toBe('00:00')
    expect(editTimeDigits('18:35', 0, 0, 'Backspace').text).toBe('18:35')
  })

  it('requires a valid time on form submission, including invalid intermediate edits', () => {
    const html = renderToStaticMarkup(createElement(MinuteField, { value: 510, change: () => {}, label: '开始时间' }))
    expect(html).toContain('required=""')
    expect(html).toContain('inputMode="numeric"')
    expect(html).toContain('value="08:30"')
    const pattern = new RegExp(`^(?:${html.match(/pattern="([^"]+)"/)![1]})$`)
    for (const text of ['00:00', '08:30', '23:59', '24:00']) expect(pattern.test(text)).toBe(true)
    for (const text of ['29:90', '24:01', '12:60', '8:30', '08:3', '08-30', '１２:３０']) expect(pattern.test(text)).toBe(false)
    expect(pattern.test(editTimeDigits('08:30', 0, 5, '2990').text)).toBe(false)
  })
})
