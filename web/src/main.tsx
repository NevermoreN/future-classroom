import { createRoot } from 'react-dom/client'
import App from './App'
import PwaInstallPrompt, { registerPwaWorker } from './PwaInstallPrompt'
import './styles.css'
import brand from './assets/icons/brand.png'
const favicon = document.createElement('link')
favicon.rel = 'icon'; favicon.type = 'image/png'; favicon.href = brand
document.head.append(favicon)
const root = document.getElementById('root')
if (!root) throw new Error('页面容器缺失')
createRoot(root).render(<><PwaInstallPrompt /><App /></>)
registerPwaWorker()
