import { test } from 'node:test'
import assert from 'node:assert/strict'
import { ShortcutCapture } from '../frontend/src/lib/shortcut-capture.ts'

const key = (code, flags = {}) => ({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, repeat: false, isComposing: false, ...flags })

test('left and right Command are committed on release', () => {
  const right = new ShortcutCapture()
  assert.equal(right.keyDown(key('MetaRight', { metaKey: true })).preview, 'RCommand')
  assert.equal(right.keyUp(key('MetaRight')).shortcut, 'RCommand')
  const left = new ShortcutCapture()
  left.keyDown(key('MetaLeft', { metaKey: true }))
  assert.equal(left.keyUp(key('MetaLeft')).shortcut, 'LCommand')
})

test('combination waits for release and preserves all modifiers', () => {
  const capture = new ShortcutCapture()
  capture.keyDown(key('ControlLeft', { ctrlKey: true }))
  capture.keyDown(key('ShiftLeft', { ctrlKey: true, shiftKey: true }))
  assert.equal(capture.keyDown(key('Space', { ctrlKey: true, shiftKey: true })).preview, 'Control+Shift+Space')
  assert.equal(capture.keyUp(key('Space', { ctrlKey: true, shiftKey: true })).done, false)
  capture.keyUp(key('ShiftLeft', { ctrlKey: true }))
  assert.equal(capture.keyUp(key('ControlLeft')).shortcut, 'Control+Shift+Space')
})

test('Command release completes capture when macOS omits the ordinary keyup', () => {
  const capture = new ShortcutCapture()
  capture.keyDown(key('MetaRight', { metaKey: true }))
  capture.keyDown(key('KeyK', { metaKey: true }))
  assert.equal(capture.keyUp(key('MetaRight')).shortcut, 'Super+K')
})

test('single key and autorepeat produce one shortcut', () => {
  const capture = new ShortcutCapture()
  capture.keyDown(key('F8'))
  capture.keyDown(key('F8', { repeat: true }))
  assert.equal(capture.keyUp(key('F8')).shortcut, 'F8')
  assert.equal(capture.keyUp(key('F8')).done, false)
})

test('two ordinary keys and unsupported keys are rejected', () => {
  const capture = new ShortcutCapture()
  capture.keyDown(key('KeyA'))
  assert.ok(capture.keyDown(key('KeyB')).error)
  capture.keyUp(key('KeyA'))
  assert.ok(capture.keyUp(key('KeyB')).error)
  const unsupported = new ShortcutCapture()
  assert.ok(unsupported.keyDown(key('Unidentified')).error)
  assert.ok(unsupported.keyUp(key('Unidentified')).error)
})

test('both Command keys cannot be mistaken for standalone right Command', () => {
  const capture = new ShortcutCapture()
  capture.keyDown(key('MetaLeft', { metaKey: true }))
  capture.keyDown(key('MetaRight', { metaKey: true }))
  capture.keyUp(key('MetaLeft', { metaKey: true }))
  assert.ok(capture.keyUp(key('MetaRight')).error)
})

for (const [code, flag, expected] of [
  ['ControlLeft', 'ctrlKey', 'LControl'], ['ControlRight', 'ctrlKey', 'RControl'],
  ['ShiftLeft', 'shiftKey', 'LShift'], ['ShiftRight', 'shiftKey', 'RShift'],
  ['AltLeft', 'altKey', 'LAlt'], ['AltRight', 'altKey', 'RAlt'],
]) {
  test(`standalone ${code} retains its physical side`, () => {
    const capture = new ShortcutCapture()
    assert.equal(capture.keyDown(key(code, { [flag]: true })).preview, expected)
    assert.equal(capture.keyUp(key(code)).shortcut, expected)
  })
}

test('Escape can be recorded as a single key', () => {
  const capture = new ShortcutCapture()
  capture.keyDown(key('Escape'))
  assert.equal(capture.keyUp(key('Escape')).shortcut, 'Escape')
})
