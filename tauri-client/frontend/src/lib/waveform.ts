export function waveformAmplitude(level: number) {
  // Map normalized amplitude from -55 dB to -8 dB onto the available height.
  return Math.max(0, Math.min(1, (20 * Math.log10(level) + 55) / 47))
}
