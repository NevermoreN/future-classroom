import { createRoot } from 'react-dom/client'
import App from './App'
import './styles.css'
const root = document.getElementById('root')
if (!root) throw new Error('页面容器缺失')
createRoot(root).render(<App />)
