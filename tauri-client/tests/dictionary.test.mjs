import { test } from 'node:test'
import assert from 'node:assert/strict'
import { validateWord, dictionaryMatches, serializeHotwords } from '../frontend/src/lib/dictionary.ts'

const entry = (text, id = text) => ({ id, text, created_at: 1, updated_at: 1 })

test('validates duplicates but allows editing the same word and preserves spelling', () => {
  const entries = [entry('OAuth', '1'), entry('ego Lite', '2')]
  assert.equal(validateWord(entries, ' oauth '), '该词汇已存在')
  assert.equal(validateWord(entries, 'OAUTH', '1'), '')
  assert.equal(validateWord(entries, 'ego Lite', '1'), '该词汇已存在')
  assert.equal(serializeHotwords(entries), 'OAuth\nego Lite')
})

test('capacity includes UTF-8 bytes and newlines, including an edited entry', () => {
  const entries = [entry('词'.repeat(333))]
  assert.equal(validateWord([], 'a'.repeat(1000)), '')
  assert.match(validateWord(entries, 'a'), /容量已满/)
  assert.equal(validateWord(entries, 'a'.repeat(1000), entries[0].id), '')
  assert.match(validateWord([], '词'.repeat(334)), /容量已满/)
})

test('rejects empty and multiline terms and searches literal partial words', () => {
  for (const word of [' ', 'a\nb', 'a\tb', 'a\u2028b']) assert.notEqual(validateWord([], word), '')
  for (const word of ['AGENTS.md', '个人/团队', 'omnibox-web', 'ego Lite']) assert.equal(validateWord([], word), '')
  assert.equal(dictionaryMatches('OmniBox', ' OMNI '), true)
  assert.equal(dictionaryMatches('赵阳', '阳'), true)
  assert.equal(dictionaryMatches('OAuth', '.*'), false)
})
