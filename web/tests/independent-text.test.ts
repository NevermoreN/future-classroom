import { describe, expect, it } from 'vitest'
import { decodeIndependentTxt, validateIndependentText } from '../src/IndependentContentPanel'
describe('independent TXT import', () => {
  it('preserves Unicode and line breaks in UTF8 and BOM-marked UTF16', () => {
    const text = '第一行\r\n第二行\t😀'
    expect(decodeIndependentTxt(new TextEncoder().encode(text))).toBe(text)
    for (const littleEndian of [true, false]) {
      const units = Array.from({ length: text.length }, (_, n) => text.charCodeAt(n)), bytes = new Uint8Array(2 + units.length * 2)
      bytes.set(littleEndian ? [255, 254] : [254, 255]); const view = new DataView(bytes.buffer)
      units.forEach((unit, n) => view.setUint16(2 + n * 2, unit, littleEndian))
      expect(decodeIndependentTxt(bytes)).toBe(text)
    }
  })
  it('rejects invalid encoding and domain capacity/control characters', () => {
    expect(() => decodeIndependentTxt(new Uint8Array([255, 1]))).toThrow()
    expect(() => decodeIndependentTxt(new Uint8Array([255, 254, 0]))).toThrow()
    expect(() => validateIndependentText('a\u0000b')).toThrow()
    expect(() => validateIndependentText('字'.repeat(10001))).toThrow()
    expect(() => validateIndependentText('\ud800')).toThrow()
  })
})
