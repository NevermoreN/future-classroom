export const installDismissKey = 'fc.pwa.install-dismissed'
export type InstallEnvironment = { userAgent: string; platform: string; touchPoints: number; standalone: boolean }
export function installGuidance(environment: InstallEnvironment): { mobile: boolean; steps: string } {
  const ua = environment.userAgent
  const ios = /iPhone|iPad|iPod/i.test(ua) || (environment.platform === 'MacIntel' && environment.touchPoints > 1)
  if (/MicroMessenger|QQ\/|DingTalk|FBAN|FBAV/i.test(ua)) return { mobile: true, steps: '请先通过浏览器菜单选择“在系统浏览器中打开”。iPhone 使用 Safari，Android 使用支持安装的浏览器，再选择添加到主屏幕。' }
  if (ios) return { mobile: true, steps: '在 Safari 中点击“分享”，选择“添加到主屏幕”，再点“添加”。若没有此选项，请用 Safari 重新打开当前地址。' }
  if (/Android/i.test(ua)) return { mobile: true, steps: '打开浏览器菜单，选择“安装应用”或“添加到主屏幕”。是否提供安装由浏览器决定；没有该选项时可以继续使用网页。' }
  return { mobile: false, steps: '打开浏览器菜单，查找“安装未来教室”或“安装此网站为应用”。浏览器未提供该功能时，可继续使用网页或收藏当前地址。' }
}
