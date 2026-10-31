import { Kbd, KbdGroup } from '@/components/ui/kbd'

const keyLabels: Record<string, string> = {
  command: '⌘', cmd: '⌘', meta: '⌘', super: '⌘',
  lcommand: '左 ⌘', lcontrol: '左 Ctrl',
  lshift: '左 ⇧', rshift: '右 ⇧', lalt: '左 Alt', ralt: '右 Alt', fn: 'Fn',
  rcommand: '右 ⌘', rightcommand: '右 ⌘',
  control: 'Ctrl', ctrl: 'Ctrl', rcontrol: '右 Ctrl', rightcontrol: '右 Ctrl',
  shift: '⇧', alt: 'Alt', option: '⌥',
  space: 'Space', escape: 'Esc', esc: 'Esc', enter: 'Enter', return: 'Enter',
  arrowup: '↑', arrowdown: '↓', arrowleft: '←', arrowright: '→',
}

export function ShortcutKeys({ shortcut }: { shortcut: string }) {
  const keys = shortcut.split('+').map(key => key.trim()).filter(Boolean)
  if (!keys.length) return null
  return <KbdGroup aria-label={shortcut} className="flex-wrap">
    {keys.map((key, index) => <Kbd key={`${index}-${key}`}>
      {keyLabels[key.toLowerCase()] ?? key}
    </Kbd>)}
  </KbdGroup>
}
