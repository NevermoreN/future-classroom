import { ProtocolFault } from './api/protocol'

export interface Classroom { address: string; schoolName: string; className: string | null; isCurrent: boolean }
export interface ClassroomDirectory { schemaVersion: 1; discoveryAvailable: boolean; rooms: Classroom[] }
export interface SavedClassroom { address: string; schoolName: string; className: string | null; favorite: boolean; recent: boolean }
export const DIRECTORY_STORAGE_KEY = 'fc.classroom-directory.v1'
const MAX_ROOMS = 64

export function classroomAddress(raw: unknown): string {
  if (typeof raw !== 'string' || raw.length > 1024 || /[\u0000-\u0020\u007f\\?#]/u.test(raw)) throw new ProtocolFault()
  let url: URL
  try { url = new URL(raw) } catch { throw new ProtocolFault() }
  const host = url.hostname.toLowerCase(), octets = host.split('.').map(Number)
  const privateV4 = /^(\d{1,3}\.){3}\d{1,3}$/.test(host) && octets.every(value => value <= 255)
    && (octets[0] === 10 || (octets[0] === 172 && octets[1]! >= 16 && octets[1]! <= 31) || (octets[0] === 192 && octets[1] === 168))
  const privateV6 = /^\[f[cd][0-9a-f]{2}:[0-9a-f:]+\]$/.test(host)
  if (url.protocol !== 'https:' || url.username || url.password || url.pathname !== '/fc/' || url.search || url.hash || (!privateV4 && !privateV6)) throw new ProtocolFault()
  return url.href
}
function label(raw: unknown, nullable = false): string | null {
  if (nullable && raw === null) return null
  if (typeof raw !== 'string' || !raw.trim() || raw.length > 512 || /[\u0000-\u001f\u007f]/u.test(raw)) throw new ProtocolFault()
  return raw
}
function record(raw: unknown): Record<string, unknown> {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new ProtocolFault()
  return raw as Record<string, unknown>
}
export function classroomDirectory(raw: unknown): ClassroomDirectory {
  const value = record(raw)
  if (value.schemaVersion !== 1 || typeof value.discoveryAvailable !== 'boolean' || !Array.isArray(value.rooms) || value.rooms.length > MAX_ROOMS) throw new ProtocolFault()
  const addresses = new Set<string>()
  const rooms = value.rooms.map(rawRoom => {
    const room = record(rawRoom), address = classroomAddress(room.address)
    if (typeof room.isCurrent !== 'boolean' || addresses.has(address)) throw new ProtocolFault()
    addresses.add(address)
    return { address, schoolName: label(room.schoolName)!, className: label(room.className, true), isCurrent: room.isCurrent }
  })
  return { schemaVersion: 1, discoveryAvailable: value.discoveryAvailable, rooms }
}
/** Only public labels and destinations are stored, never account or API payloads. */
export function readSavedClassrooms(storage: Pick<Storage, 'getItem'>): SavedClassroom[] {
  try {
    const text = storage.getItem(DIRECTORY_STORAGE_KEY)
    if (!text || text.length > 200_000) return []
    const value = record(JSON.parse(text))
    if (value.schemaVersion !== 1 || !Array.isArray(value.rooms) || value.rooms.length > MAX_ROOMS) return []
    const seen = new Set<string>()
    return value.rooms.map(raw => {
      const room = record(raw), address = classroomAddress(room.address)
      if (seen.has(address) || typeof room.favorite !== 'boolean' || typeof room.recent !== 'boolean') throw new ProtocolFault()
      seen.add(address)
      return { address, schoolName: label(room.schoolName)!, className: label(room.className, true), favorite: room.favorite, recent: room.recent }
    })
  } catch { return [] }
}
export function saveClassrooms(storage: Pick<Storage, 'setItem'>, rooms: SavedClassroom[]): boolean {
  const publicRooms = rooms.slice(0, MAX_ROOMS).map(({ address, schoolName, className, favorite, recent }) => ({ address, schoolName, className, favorite, recent }))
  try { storage.setItem(DIRECTORY_STORAGE_KEY, JSON.stringify({ schemaVersion: 1, rooms: publicRooms })); return true } catch { return false }
}
export function mergeClassrooms(rooms: Classroom[], saved: SavedClassroom[]): SavedClassroom[] {
  const current = rooms.map(room => {
    const old = saved.find(item => item.address === room.address)
    return { address: room.address, schoolName: room.schoolName, className: room.className, favorite: old?.favorite ?? false, recent: old?.recent ?? false }
  })
  return [...current, ...saved.filter(old => !rooms.some(room => room.address === old.address))]
    .sort((a, b) => Number(b.favorite) - Number(a.favorite) || Number(b.recent) - Number(a.recent)
      || (a.recent ? saved.findIndex(item => item.address === a.address) - saved.findIndex(item => item.address === b.address) : 0)).slice(0, MAX_ROOMS)
}
