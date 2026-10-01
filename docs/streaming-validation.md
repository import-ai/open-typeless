# Streaming ASR scenario validation

Validated on 2026-10-01 on macOS. Automated tests exercise production audio,
HTTP, session guards, WAV files, and SQLite storage in isolated temporary
directories. They do not invoke the physical microphone or native paste.

## Scenario results

| Scenario | Evidence and result |
| --- | --- |
| Streaming capability present, absent, or false | Rust health tests select streaming only for explicit `true`; older responses retain the 12 MiB default. Go health publishes the configured audio limit. |
| Audio sent before recording ends | Rust TCP tests and Go full-duplex tests observe PCM/partial results before EOF; only an explicit final is accepted. |
| Local recording and archive under streaming | `recording_to_history_scenarios_preserve_audio_and_count_once` compares live PCM, the complete fallback WAV, and archived bytes. Both real-ASR smoke runs also archive the WAV and remove the temporary file. |
| Leading silence, pre-roll, and microphone tail | `recording` tests compare exact samples at 16 kHz mono and 48 kHz stereo across arbitrary chunk boundaries, including the tail. Silence is rejected without starting streaming. Physical microphone tail timing remains unverified. |
| Old endpoint, mid-capture disconnect, disconnect after EOF | Eight combinations of normal streaming, these three alternatives, and temporary-write success/failure pass. Fallback uploads the complete WAV with the original hotwords. |
| Unsupported endpoint, malformed reply, missing final | Existing transport tests fall back to multipart. An early final while capture is open is rejected. Fragmented UTF-8/JSON is parsed correctly. |
| Slow upload | A deliberately undrained bounded queue aborts the stream without blocking capture; the full WAV remains available. This is deterministic backpressure injection, not a bandwidth-shaped network test. |
| Temporary WAV write failure or existing destination | Recognition and history use in-memory WAV bytes when publication fails. Atomic publication does not overwrite an existing file or leave a staging file. |
| History transaction failure | Injected SQLite failure rolls back history and usage, removes the incomplete archive, and preserves the source WAV. Recovery through the in-memory save path preserves exact bytes. Production reports a recovery path when a temporary WAV exists. |
| Cancellation during upload or after stop | Tests abort waiting, reject late results through the session guard, clean temporary audio, and leave history/usage empty. Cancellation never triggers offline retry. |
| Repeated transcription and superseded sessions | Session tests reject duplicate transcription and stale commits. Accepted results complete once; history is idempotent. Automatic-stop notifications also check the current session on the main thread. |
| History duration, statistics, deletion, and restart | The eight-case matrix verifies a 700 ms WAV duration, one usage increment per accepted result, byte-identical archived audio, deletion without decrementing usage, and persistence after reopening SQLite. |
| Dictionary and settings changed during capture | Tests retain the start-of-recording hotword snapshot and verify authorization headers. Source review confirms the URL/key pair and dictionary are captured together in the run before health/capture. |
| Authentication, redirects, and input limits | Go/Rust tests cover bearer auth, rejected redirects, invalid PCM metadata, incomplete frames, oversized hotwords/audio, and configured health limits. |
| Polishing and partial results | Go tests polish the final transcript once; existing shared-polisher tests cover failure, incomplete output, and cancellation. Rust prefers nonempty polished text and supports raw-only older servers. |
| Recording size and automatic completion | Audio tests enforce bounded memory/WAV size and complete frames. A regression test retrieves completed capture even when its stop receiver is closed. The four-minute timer, microphone-error stop, and UI stop-during-start handling were reviewed and compiled; hardware interaction remains unverified. |
| Empty transcript and final cleanup | Source review confirms empty raw text skips history/paste and cleans temporary audio. File cleanup is exercised by success/cancellation tests. No native empty-transcript session was performed. |
| Pill, shortcuts, microphone permission, and native paste | Native debug app builds successfully, but UI automation reports that the Mac is locked and automatic unlock failed. Focus, clipping, dragging, Esc, device unplug, sleep/resume, playback, and actual paste require an unlocked desktop. Windows was not run. |

## Fixes made during validation

- Share bounded, trimmed samples between live PCM, fallback WAV, and history;
  preserve the 100 ms pre-roll and queued tail.
- Stop capture at the advertised byte limit (at most 12 MiB including WAV
  header), four minutes, or a microphone error, and report the reason. A stopped
  worker still returns its result; a delayed auto-stop cannot target a newer run.
- Publish temporary WAVs atomically. Retain bytes for upload/archive on write
  failure, and retain existing WAVs with recovery paths on recognition/history
  failure. If both temporary and history storage fail, no durable audio copy is
  guaranteed; the storage warning remains visible.
- Bound the offline request to 60 seconds and keep late microphone events from
  replacing the pill's processing/error state.

## Reproduction

From the repository root:

```sh
go test -race ./...
go vet ./...
```

From `tauri-client/`:

```sh
npm run build
node --experimental-strip-types --test tests/*.test.mjs
cargo test --locked
npm run tauri build -- --debug --bundles app
```

Results: Go tests/race detector and vet passed; frontend production build and
19 Node tests passed; Rust reported 37 passed and one opt-in test ignored.
The opt-in test was then run separately with a local Business Server connected
to the running GPU audio.cpp `r2t2-asr-stream` model, using the public Qwen Chinese
sample in 16 kHz mono and 48 kHz stereo. Both returned the expected transcript,
saved byte-identical archive WAVs, removed temporary recordings, and counted
one recognition. Offline inference through the original endpoint was also
checked with that sample. These runs use file-driven capture, not the microphone.

To repeat the live test, set `OPEN_TYPELESS_TEST_BASE_URL` to the complete API
base, `OPEN_TYPELESS_TEST_API_KEY` to a disposable test key, and
`OPEN_TYPELESS_TEST_WAV` to an s16 WAV. Optionally set
`OPEN_TYPELESS_TEST_EXPECTED` to the expected raw transcript, then run:

```sh
cargo test --locked streaming::tests::live_backend_smoke -- --ignored
```

The live test requires a streaming final directly; offline fallback cannot hide
a failed stream. It writes only to its own temporary history directory.
