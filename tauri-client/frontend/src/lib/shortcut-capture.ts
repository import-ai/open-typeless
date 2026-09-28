type KeyEvent = Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey' | 'repeat' | 'isComposing'>

const modifier = /^(Control|Alt|Shift|Meta)(Left|Right)$|^Fn$/
const modifierNames: Record<string, string> = {
  MetaLeft: 'LCommand', MetaRight: 'RCommand',
  ControlLeft: 'LControl', ControlRight: 'RControl',
  ShiftLeft: 'LShift', ShiftRight: 'RShift',
  AltLeft: 'LAlt', AltRight: 'RAlt', Fn: 'Fn',
}
const supportedKey = /^(Escape|Key[A-Z]|Digit[0-9]|F([1-9]|1[0-9]|2[0-4])|Numpad([0-9]|Add|Decimal|Divide|Enter|Equal|Multiply|Subtract)|Backquote|Backslash|BracketLeft|BracketRight|Comma|Equal|Minus|Period|Quote|Semicolon|Slash|Backspace|CapsLock|Enter|Space|Tab|Delete|End|Home|Insert|PageDown|PageUp|PrintScreen|ScrollLock|Arrow(Down|Left|Right|Up)|NumLock|Pause)$/

export class ShortcutCapture {
  private held = new Set<string>()
  private ordinary = new Set<string>()
  private modifiers = new Set<string>()
  private candidate = ''
  private invalid = false

  keyDown(event: KeyEvent): { preview: string; error?: string } {
    if (event.repeat) return { preview: this.candidate }
    this.held.add(event.code)
    const modifiers = [event.ctrlKey && 'Control', event.altKey && 'Alt', event.shiftKey && 'Shift', event.metaKey && 'Super'].filter(Boolean) as string[]
    if (modifier.test(event.code)) {
      this.modifiers.add(event.code)
      if (!this.ordinary.size) {
        this.candidate = this.modifiers.size === 1 ? modifierNames[event.code] : modifiers.join('+')
      }
    } else {
      this.ordinary.add(event.code)
      if (event.isComposing || !supportedKey.test(event.code) || this.ordinary.size > 1) {
        this.invalid = true
        return { preview: this.candidate, error: '请按一个普通按键，可搭配 Command、Ctrl、Alt、Shift。' }
      }
      const key = event.code.replace(/^Key(?=[A-Z]$)|^Digit(?=[0-9]$)/, '')
      this.candidate = [...modifiers, key].join('+')
    }
    return { preview: this.candidate }
  }

  keyUp(event: KeyEvent): { done: boolean; shortcut?: string; error?: string } {
    if (!this.held.size) return { done: false }
    this.held.delete(event.code)
    // macOS may omit ordinary keyup events while Command is held.
    if (event.code.startsWith('Meta') && !event.metaKey) {
      for (const key of this.held) if (!modifier.test(key)) this.held.delete(key)
    }
    if (this.held.size || event.ctrlKey || event.altKey || event.shiftKey || event.metaKey) return { done: false }
    if (this.invalid || (!this.ordinary.size && this.modifiers.size !== 1) || !this.candidate) {
      return { done: true, error: '请按一个单键，或修饰键加一个普通按键。' }
    }
    return { done: true, shortcut: this.candidate }
  }
}
