import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import ClassroomHome from '../src/ClassroomHome'
import { classroomAddress, classroomDirectory, DIRECTORY_STORAGE_KEY, mergeClassrooms, readSavedClassrooms, saveClassrooms, type SavedClassroom } from '../src/classroom-directory'

const room = { address: 'https://192.168.10.212:8443/fc/', schoolName: '学校', className: '三年二班', isCurrent: true }
describe('public classroom directory boundary', () => {
  it('accepts school private addresses and rejects credentials, public hosts and alternate routes', () => {
    for (const address of ['https://10.0.0.2/fc/', 'https://172.16.0.2:8443/fc/', 'https://[fd12::2]:8443/fc/']) expect(classroomAddress(address)).toBe(address)
    for (const address of ['https://127.0.0.1/fc/', 'http://192.168.1.1/fc/', 'https://school.example/fc/', 'https://8.8.8.8/fc/', 'https://172.32.0.1/fc/', 'https://[fe80::1]/fc/', 'https://user:secret@192.168.1.1/fc/', 'https://192.168.1.1/fc/?secret=1', 'https://192.168.1.1/fc/#', 'https://192.168.1.1/admin/']) expect(() => classroomAddress(address)).toThrow()
  })
  it('bounds and validates server lists without turning saved rooms into discovery results', () => {
    const response = { schemaVersion: 1, discoveryAvailable: true, rooms: [room] }
    expect(classroomDirectory(response).rooms).toEqual([room])
    expect(() => classroomDirectory({ ...response, rooms: [room, room] })).toThrow()
    expect(() => classroomDirectory({ ...response, rooms: Array(65).fill(room) })).toThrow()
    expect(() => classroomDirectory({ ...response, rooms: [{ ...room, schoolName: '\nunsafe' }] })).toThrow()
    const old: SavedClassroom = { address: 'https://10.0.0.5/fc/', schoolName: '学校', className: '一班', favorite: true, recent: true }
    const merged = mergeClassrooms([room], [old])
    expect(merged).toContainEqual(old)
    expect(merged).toHaveLength(2)
    expect(merged.every(item => !('isCurrent' in item))).toBe(true)
  })
  it('persists only public labels, bounded bookmarks and recent destinations', () => {
    let stored = ''
    const storage = { getItem: (key: string) => key === DIRECTORY_STORAGE_KEY ? stored : null, setItem: (_key: string, value: string) => { stored = value } }
    const saved = { ...room, favorite: true, recent: false, account: 'not-stored', token: 'not-stored' }
    expect(saveClassrooms(storage, [saved])).toBe(true)
    expect(stored).not.toContain('account')
    expect(stored).not.toContain('token')
    expect(stored).not.toContain('isCurrent')
    expect(readSavedClassrooms(storage)).toEqual([{ address: room.address, schoolName: room.schoolName, className: room.className, favorite: true, recent: false }])
    stored = JSON.stringify({ schemaVersion: 1, rooms: [{ ...saved, address: 'https://evil.example/fc/' }] })
    expect(readSavedClassrooms(storage)).toEqual([])
    stored = JSON.stringify({ schemaVersion: 2, rooms: [saved] })
    expect(readSavedClassrooms(storage)).toEqual([])
    expect(saveClassrooms({ setItem: () => { throw new Error('disabled') } }, [saved])).toBe(false)
  })
  it('shows a persistent installation entry and explains classroom and offline boundaries', () => {
    const html = renderToStaticMarkup(createElement(ClassroomHome, { onInstallHelp: () => undefined }))
    expect(html).toContain('选择班级后打开该教室网站')
    expect(html).toContain('安装到手机 / 查看步骤')
    expect(html).toContain('离线时可以查看已保存名单')
    expect(html).toContain('各教室分别登录')
    expect(html).not.toContain('<iframe')
    const installed = renderToStaticMarkup(createElement(ClassroomHome, { installed: true, onInstallHelp: () => undefined }))
    expect(installed).not.toContain('安装到手机 / 查看步骤')
  })
})
