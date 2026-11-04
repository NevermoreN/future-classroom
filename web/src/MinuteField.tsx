import { useEffect, useRef, useState } from 'react'
import { Input } from '@/components/ui/input'

const validTime = /^(?:[01][0-9]|2[0-3]):[0-5][0-9]$|^24:00$/
export function minuteText(value: number) { return `${Math.floor(value / 60).toString().padStart(2, '0')}:${(value % 60).toString().padStart(2, '0')}` }

// Digits are overwritten in place; the separator and four digit positions always remain.
export function editTimeDigits(text: string, start: number, end: number, edit: string | 'Backspace' | 'Delete') {
  const chars = text.split(''), positions = [0, 1, 3, 4]
  if (edit === 'Backspace' || edit === 'Delete') {
    const selected = positions.filter(p => p >= start && p < end)
    const at = edit === 'Backspace' ? positions.filter(p => p < start).at(-1) : positions.find(p => p >= start)
    for (const p of selected.length ? selected : at === undefined ? [] : [at]) chars[p] = '0'
    return { text: chars.join(''), caret: selected.length ? start : edit === 'Backspace' ? at ?? 0 : start }
  }
  let caret = start === 2 ? 3 : start
  for (const digit of edit) {
    if (caret > 4) break
    chars[caret] = digit
    caret += caret === 1 ? 2 : 1
  }
  return { text: chars.join(''), caret }
}

export default function MinuteField({ value, change, label }: { value: number; change: (n: number) => void; label: string }) {
  const [text, setText] = useState(minuteText(value)), input = useRef<HTMLInputElement>(null)
  useEffect(() => setText(minuteText(value)), [value])
  function update(next: string) {
    setText(next)
    if (validTime.test(next)) { const [hours, minutes] = next.split(':').map(Number); change(hours! * 60 + minutes!) }
  }
  function edit(digits: string | 'Backspace' | 'Delete') {
    const field = input.current
    if (!field) return
    const next = editTimeDigits(text, field.selectionStart ?? 0, field.selectionEnd ?? 0, digits)
    update(next.text)
    requestAnimationFrame(() => field.setSelectionRange(next.caret, next.caret))
  }
  useEffect(() => {
    const field = input.current
    if (!field) return
    // Use the native event: React's beforeInput fallback may omit inputType
    // and does not forward every mobile deletion event.
    function beforeInput(event: InputEvent) {
      const kind = event.inputType ?? ''
      if (event.data) { event.preventDefault(); if (/^[0-9]+$/.test(event.data)) edit(event.data) }
      else if (kind.startsWith('delete')) { event.preventDefault(); edit(kind.includes('Backward') ? 'Backspace' : 'Delete') }
    }
    field.addEventListener('beforeinput', beforeInput)
    return () => field.removeEventListener('beforeinput', beforeInput)
  }, [text, change])
  return <Input ref={input} className="minute-field" aria-label={label} title="24小时制，直接输入数字；例如08:30，结束时间可用24:00" inputMode="numeric" autoComplete="off" value={text} pattern="(?:[01][0-9]|2[0-3]):[0-5][0-9]|24:00" required onFocus={e => e.target.select()} onKeyDown={e => {
    if (e.ctrlKey || e.metaKey || e.altKey || e.nativeEvent.isComposing || e.key === 'Unidentified' || e.key === 'Process' || e.key === 'Tab' || e.key.startsWith('Arrow') || e.key === 'Home' || e.key === 'End' || e.key === 'Enter') return
    e.preventDefault()
    if (/^[0-9]$/.test(e.key) || e.key === 'Backspace' || e.key === 'Delete') edit(e.key)
  }} onPaste={e => {
    e.preventDefault()
    const pasted = e.clipboardData.getData('text').trim()
    if (/^[0-9]{2}:[0-9]{2}$/.test(pasted)) { update(pasted); return }
    if (/^[0-9]{1,4}$/.test(pasted)) edit(pasted)
  }} onChange={e => { if (/^[0-9]{2}:[0-9]{2}$/.test(e.target.value)) update(e.target.value) }} />
}
