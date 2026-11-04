import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Section, Toggle } from './AdminForms'
export default function RecoveryCodeNotice({ code, onDone }: { code: string; onDone: () => void }) {
  const [saved, setSaved] = useState(false)
  return <Section title="请保存这次生成的恢复码" description="恢复码只展示这一次。请离线妥善保存；忘记密码时，可在白板本机使用它设置新密码。"><code className="recovery-code">{code}</code><p>若这是轮换或恢复后的新码，旧码已经失效。页面不保存恢复码，关闭或刷新后无法再次查看。</p><Toggle label="我已妥善保存这次恢复码" value={saved} onChange={setSaved} /><Button type="button" disabled={!saved} onClick={onDone}>完成并隐藏恢复码</Button></Section>
}
