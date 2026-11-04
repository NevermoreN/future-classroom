/** Product-controlled icons come from imagegen assets. No runtime icon CDN. */
import type { CSSProperties, ImgHTMLAttributes } from 'react'
import book from '../assets/icons/book.png'
import folder from '../assets/icons/folder.png'
import calendar from '../assets/icons/calendar.png'
import settings from '../assets/icons/settings.png'
import account from '../assets/icons/account.png'
import microphone from '../assets/icons/microphone.png'
import arrow from '../assets/icons/arrow.png'
import check from '../assets/icons/check.png'
import alert from '../assets/icons/alert.png'
import refresh from '../assets/icons/refresh.png'
import close from '../assets/icons/close.png'
import transfer from '../assets/icons/transfer.png'
import remove from '../assets/icons/delete.png'
import home from '../assets/icons/home.png'
import power from '../assets/icons/power.png'
import layout from '../assets/icons/layout.png'
import brand from '../assets/icons/brand.png'
import plus from '../assets/icons/plus.png'
import minus from '../assets/icons/minus.png'

type Props = Omit<ImgHTMLAttributes<HTMLImageElement>, 'src' | 'width' | 'height'> & { size?: number | string; strokeWidth?: number }
function icon(src: string, transform?: string) {
  return function GeneratedIcon({ size = 20, strokeWidth: _strokeWidth, className = '', style, ...props }: Props) {
    const dimensions: CSSProperties = { width: size, height: size, flexShrink: 0, objectFit: 'contain', ...(transform ? { transform } : {}), ...style }
    return <img {...props} className={`fc-icon ${className}`} src={src} alt="" aria-hidden="true" style={dimensions} />
  }
}
export const BookOpen = icon(book), ClipboardList = icon(book), FolderOpen = icon(folder)
export const Clock3 = icon(calendar), History = icon(calendar), CalendarDays = icon(calendar)
export const Settings = icon(settings), UserRound = icon(account), UsersRound = icon(account), ShieldCheck = icon(account), LockKeyhole = icon(account)
export const AudioLines = icon(microphone), Mic = icon(microphone), Radio = icon(microphone), Volume2 = icon(microphone)
export const ArrowRight = icon(arrow), ChevronRight = icon(arrow), ArrowLeft = icon(arrow, 'rotate(180deg)')
export const Check = icon(check), Save = icon(check), CircleAlert = icon(alert), Info = icon(alert)
export const RefreshCw = icon(refresh), LoaderCircle = icon(refresh), X = icon(close)
export const Upload = icon(transfer, 'rotate(180deg)'), FileUp = icon(transfer, 'rotate(180deg)'), Download = icon(transfer)
export const Trash2 = icon(remove), MonitorUp = icon(home), Smartphone = icon(home), LogOut = icon(power), Square = icon(power)
export const Sparkles = icon(layout), GraduationCap = icon(brand)
export const Plus = icon(plus), Minus = icon(minus)
