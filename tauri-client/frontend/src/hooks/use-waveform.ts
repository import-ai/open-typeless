import { useEffect, useState } from 'react'

const silence: readonly number[] = Array<number>(9).fill(0)
const previewFrames = [
  [0, 0.008, 0.025, 0.08, 0.04, 0.015, 0.03, 0.06, 0.05],
  [0.12, 0.08, 0.045, 0.015, 0.01, 0.024, 0.036, 0.02, 0.009],
  [0.006, 0.012, 0.02, 0.008, 0.01, 0.035, 0.024, 0.055, 0.09],
  [0.045, 0.02, 0.025, 0.034, 0.018, 0.01, 0.005, 0, 0],
  silence,
  silence,
]

export function useWaveform(active: boolean, preview: boolean) {
  const [levels, setLevels] = useState(silence)

  useEffect(() => {
    setLevels(silence)
    if (!active || !preview) return

    let previewIndex = 0
    const timer = setInterval(() => {
      setLevels(previewFrames[previewIndex++ % previewFrames.length])
    }, 100)
    return () => clearInterval(timer)
  }, [active, preview])

  return {
    levels,
    showLevels(next: readonly number[]) {
      if (active && !preview) setLevels(next)
    },
  }
}
