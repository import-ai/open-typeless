import { test } from 'node:test'
import assert from 'node:assert/strict'
import { waveformAmplitude } from '../frontend/src/lib/waveform.ts'

test('waveform responds to quiet levels without saturating at moderate levels', () => {
  const [silence, quiet, moderate, loud] = [0, 0.025, 0.1, 1].map(waveformAmplitude)
  assert.equal(silence, 0)
  assert.ok(quiet > silence && moderate > quiet && moderate < loud)
  assert.equal(loud, 1)
})
