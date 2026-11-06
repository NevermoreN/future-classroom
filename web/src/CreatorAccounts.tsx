import xiaohongshuQr from './assets/social/xiaohongshu-original.png'
import douyinQr from './assets/social/douyin-original.png'
import './creator-accounts.css'

export default function CreatorAccounts() {
  return <section className="creator-accounts" aria-label="关注作者与反馈建议">
    <div className="creator-accounts-intro"><strong><span aria-hidden="true">📣</span> 关注 Eliora哟</strong><p>关注最新动态，<br className="creator-desktop-break" />欢迎提出意见与建议 <span aria-hidden="true">💬</span></p></div>
    <div className="creator-platforms">
      <div className="creator-platform creator-xiaohongshu">
        <div className="creator-platform-details"><span className="creator-platform-label">小红书</span><strong>Eliora哟</strong><span className="creator-scan-caption">扫一扫关注</span></div>
        <a className="creator-qr-link" href={xiaohongshuQr} target="_blank" rel="noopener noreferrer" aria-label="查看小红书 Eliora哟的原始二维码"><svg className="creator-account-qr" viewBox="335 21 165 165" role="img" aria-label="小红书 Eliora哟的关注二维码"><image href={xiaohongshuQr} width={531} height={195} /></svg></a>
      </div>
      <div className="creator-platform creator-douyin">
        <div className="creator-platform-details"><span className="creator-platform-label">抖音</span><strong>Eliora哟</strong><span className="creator-scan-caption">扫一扫关注</span></div>
        <a className="creator-qr-link" href={douyinQr} target="_blank" rel="noopener noreferrer" aria-label="查看抖音 Eliora哟的原始二维码"><svg className="creator-account-qr" viewBox="46 66 488 480" role="img" aria-label="抖音 Eliora哟的关注二维码"><image href={douyinQr} width={570} height={731} /></svg></a>
      </div>
    </div>
  </section>
}
