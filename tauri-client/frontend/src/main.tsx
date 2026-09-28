import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { windowLabel } from '@/lib/desktop'
import { MainWindow } from '@/windows/main-window'
import { PillWindow } from '@/windows/pill'
import { PillDebug } from '@/windows/pill-debug'
import './styles.css'
import './pill.css'

const overlay = windowLabel === 'pill' || windowLabel === 'pill-debug'
document.documentElement.dataset.window = overlay ? windowLabel : 'main'
const content = windowLabel === 'pill-debug' ? <PillDebug /> : windowLabel === 'pill' ? <PillWindow /> : <MainWindow />
createRoot(document.getElementById('root')!).render(<StrictMode>{content}</StrictMode>)
