# Project collaboration guide

## Current implementation and boundaries

Open Typeless is a desktop voice input tool: a shortcut starts recording, audio is streamed when the backend advertises support (otherwise the completed WAV is uploaded), and the final recognized text is inserted into the current application through the clipboard and a simulated paste. The client does not load an ASR model.

- Desktop: Tauri 2 + Rust, with React + TypeScript + Vite + Tailwind CSS + shadcn/ui on the frontend.
- Business service: a Go HTTP server that forwards multipart audio to a separate ASR service, then optionally polishes its transcript through an OpenAI-compatible LLM.
- Live ASR uses audio.cpp `/v1/audio/transcriptions/live` when `INFERENCE_STREAM_MODEL` names a streaming model. Authenticated health checks advertise `capabilities.streaming_asr`; the client uses `/recognitions/stream` and falls back to the complete WAV on failure. Only explicit final results may be accepted, archived, or pasted.
- Offline inference uses HTTP: legacy `/transcribe` or audio.cpp `/v1/audio/transcriptions`, selected by `INFERENCE_PROTOCOL`. `LLM_BASE_URL` enables the approved 3-shot polishing prompt; failures fall back to raw text. The client pastes nonempty `polished_text`, falling back to `raw_text` for older servers.
- `prd.md` describes product goals, including gRPC that is not implemented yet. Use source code and configuration to determine current behavior; do not describe planned features as implemented.
- Keep `README.md` focused on end-user installation and usage. Put development, debugging, and build details in `docs/development.md` and frontend conventions in `docs/frontend.md`. Server deployment and API notes are archived locally in `docs/archive/deployment.md`, which is ignored by Git. Local ASR experiments are documented in `deploy/asr/README.md`. Deployment documentation includes historical experiments; the current `deploy/asr/compose.yaml` uses R2T2. Do not infer that Qwen is running from an image name alone, or treat repository configuration as proof of remote runtime state.

## Code map

| Path | Responsibility |
| --- | --- |
| `cmd/server/` | Business API, inference forwarding, and Go tests |
| `cmd/debug-server/` | Local debug server that saves uploaded audio and always returns `foo`, without calling ASR |
| `internal/buildinfo/` | Server version reported by health checks |
| `tauri-client/src/main.rs` | Tauri commands, recording, recognition sessions, cancellation, pasting, windows, and settings integration |
| `tauri-client/src/recording.rs` | Bounded audio capture, shared silence trimming, WAV encoding, and atomic temporary files |
| `tauri-client/src/streaming.rs`, `cmd/server/stream.go` | Live PCM upload, streaming results, and audio.cpp live inference forwarding |
| `tauri-client/src/modifier_shortcut.rs` | Native macOS / Windows modifier listeners and standalone key detection |
| `tauri-client/src/settings_file.rs`, `dictionary.rs` | Settings and dictionary validation, persistence, and Rust tests |
| `tauri-client/frontend/src/windows/` | Main, pill, and pill-debug windows |
| `tauri-client/frontend/src/components/` | Application components; `ui/` contains shadcn primitives |
| `tauri-client/frontend/src/lib/desktop.ts` | Tauri command wrappers, shared types, and browser preview branches |
| `tauri-client/frontend/src/hooks/` | Event subscription cleanup and shortcut suspension during text input |
| `tauri-client/tests/` | Dictionary and shortcut tests using the Node built-in test runner |
| `tauri-client/capabilities/`, `tauri.conf.json` | Window permissions, application configuration, and packaging |
| `deploy/asr/` | Experimental ASR services, Compose configuration, probes, and performance records |
| `.github/workflows/`, `Dockerfile` | Desktop installer and business service container builds |

The Rust project lives directly in `tauri-client/`, not the usual `src-tauri/` layout. JavaScript dependencies and the lockfile also live in `tauri-client/`. The Vite root is `frontend/`, output goes to `tauri-client/dist/`, and `@/` resolves to `frontend/src/`.

## Development and validation

The Go module declares Go 1.23; CI and Docker builds currently use Go 1.25. Desktop development requires Node 22.12+, Rust, and the platform dependencies for Tauri 2. Native shortcuts and pasting primarily target macOS / Windows.

Run the business service from the repository root:

```sh
BACKEND_API_KEY=local-development-key INFERENCE_URL=http://localhost:18080 go run ./cmd/server
go test ./...
go vet ./...
```

`BACKEND_API_KEY` is required and protects health and recognition using bearer authentication. `HTTP_ADDR` defaults to `:8080`, `INFERENCE_URL` to `http://localhost:18080`, and `MAX_AUDIO_BYTES` to 12 MiB. Use `go run ./cmd/debug-server` instead of the business service to debug uploads; it uses the same default port and saves audio to `/tmp/open-typeless-debug`.

Run these commands from `tauri-client/`:

```sh
npm ci
npm run tauri dev       # Native desktop development; starts Vite automatically
npm run dev             # Browser preview only
npm run tauri:debug     # Native preview of five pill states and developer options
npm run typecheck
npm run build           # TypeScript checks and frontend production build
node --experimental-strip-types --test tests/*.test.mjs
cargo test --locked
npm run tauri build     # Desktop packaging; builds the frontend automatically
```

- Match validation to the change: run Go tests and vet for Go changes; run build for frontend changes, plus Node tests for dictionary or shortcut logic; run Cargo tests for Rust changes. There is no `npm test` script.
- If standalone Rust builds or tests report a missing `dist/`, run `npm run build` first. Cargo tests also require native platform dependencies.
- For visual changes, inspect native windows with `npm run tauri:debug` and capture screenshots. `http://localhost:5173/?view=pill-debug` is only a browser preview and cannot validate native transparency, focus, or clipping.
- Changes to recording, permissions, shortcuts, or pasting also require native end-to-end checks. macOS requires microphone and accessibility permissions. Explicitly report checks that could not be performed; compilation does not prove interactive behavior works.

## Behavior constraints to preserve

### API and recognition sessions

- The client stores a complete API base URL, such as `http://127.0.0.1:8080/api/v1`. Append only `/health`, `/recognitions`, or `/recognitions/stream`; do not duplicate `/api/v1`.
- Backend requests send the configured API key in `Authorization: Bearer ...`; never put it in URLs or logs. The server fails startup without `BACKEND_API_KEY`. Keep the URL/key pair atomic and reject client HTTP redirects.
- The backend URL is empty by default, and recording is disabled until it is configured. Check health every 30 seconds while idle and before starting a recording.
- `POST /api/v1/recognitions` accepts `audio` and optional `language` and `hotwords`. Legacy ASR uses raw-body `/transcribe` with dictionary `context`; audio.cpp uses multipart `/v1/audio/transcriptions` with dictionary `prompt`. Omit `language=auto` for audio.cpp. Preserve `raw_text` separately from LLM output; incomplete or failed polishing must fall back to raw text.
- When changing an interface, check the Go response, Rust serialization types, `frontend/src/lib/desktop.ts`, and their callers together.
- Live PCM, fallback WAVs, and archived audio must use the same trimmed samples and microphone tail. Respect health's `limits.max_audio_bytes` (12 MiB local ceiling, WAV header included); stop at that limit or four minutes. Temporary write failures retain bounded audio for fallback/history; recognition/history failures retain an existing temporary WAV with a recovery path. Cancellation and empty/successful results clean up their temporary audio.
- Cancellation must stop the client from waiting and prevent results from cancelled or superseded sessions from reaching the clipboard or being pasted. Preserve session ID checks and temporary recording cleanup; hiding the UI alone is insufficient.

### Shortcuts and windows

- The default is standalone right Command on macOS and standalone right Control on Windows. Standalone modifiers trigger on release; using them with another key, modifier, or mouse action must not accidentally start recording. Preserve platform conditional compilation boundaries.
- Suspend the activation shortcut during shortcut capture and dictionary text input. Preserve Chinese IME composition handling: Enter used to confirm a candidate must not submit a dictionary entry.
- Esc cancels recording or recognition while the pill is visible; release the cancellation shortcut when the pill is hidden. Preserve this priority even when Esc is configured as the activation key.
- The production pill does not take keyboard focus and supports first-click interaction and dragging. Retain its dragged position for the current application session.
- Use separate elements with fixed dimensions for the pill capsule and recognition circle, switching their opacity to avoid residual clipping in native macOS windows. Check both `processing -> disconnected` and `processing -> ready -> disconnected` transitions.
- Mount the main, pill, and pill-debug components by window label. Apply transparent backgrounds only to pill windows. Previews share the production pill component and do not start the microphone, ASR, or pasting.
- Use `@tauri-apps/api`, not `window.__TAURI__`. Clean up event subscriptions correctly under React StrictMode, including asynchronous subscriptions that complete after unmounting.

### Settings and dictionary

- Store settings and the dictionary in `~/.open-typeless/` on macOS and `%USERPROFILE%\.open-typeless\` on Windows, independently of the application identifier. Files in the old system application directory are not migrated automatically.
- Preserve atomic writes, error feedback, and existing data on failure. Never silently overwrite a corrupt dictionary. Browser dictionaries use separate localStorage and do not modify desktop files.
- Duplicate detection ignores English letter case. Editing retains an entry's position; new entries go first. Select-all after a search affects only current results, and changing the search clears the selection.
- The complete dictionary joined with newlines is limited to **1000 UTF-8 bytes**, including separators, not 1000 characters. Keep frontend, Rust, and Go validation consistent; never truncate silently.
- Capture a dictionary snapshot at the start of each recording. Changes take effect on the next recording. Hotwords are ASR hints, not forced replacements in recognition results.

## Build and maintenance

- `tauri-client/icons/icon.svg` is the single source for the application icon. Preserve the separate background, waveform, and cat-head groups. `tauri dev`, `tauri build`, and desktop CI generate PNG, macOS ICNS, and Windows ICO files in the Git-ignored `tauri-client/icons/generated/` directory. Packaging must reference only that directory; do not maintain or commit generated files manually. Run `npm run generate:icons` from `tauri-client/` to generate icons separately or before invoking Cargo builds/tests directly. Old PNGs, design variants, and historical exports are archived in `docs/archive/icons/` for reference only; they do not participate in builds and must not become a second icon source.
- Add shadcn components by running `npx shadcn@latest add <component>` from `tauri-client/`, following the existing theme and component directories.
- Update `package-lock.json` / `Cargo.lock` when changing dependencies. Do not implement features by manually editing `dist/`, `target/`, `node_modules/`, or Tauri-generated schemas.
- Desktop CI builds macOS arm64 / amd64 DMGs and a Windows amd64 NSIS EXE installer. Artifact filenames use `open-typeless-v<version>-<arch>.<extension>` on `v*` tags and `open-typeless-v<version>-<run_id><attempt_id>-<arch>.<extension>` otherwise. Installers are uploaded directly without ZIP wrappers. Code signing and notarization are not configured yet.
- When releasing the client, synchronize the application version in `tauri.conf.json`, `Cargo.toml`, and `Cargo.lock`. The server version lives in `internal/buildinfo/version.go`.
- The root Dockerfile packages only the Go Business Server, not ASR. The container's `INFERENCE_URL` must be reachable from inside the container.
- Update relevant documentation when changing startup commands, configuration, or user behavior. Preserve uncommitted changes present at task start and avoid unrelated refactoring or formatting.

## Project language

Use English for all written project materials, including:

- Commit messages, including their subjects and bodies.
- Pull request titles and descriptions, issue text, review feedback, and discussion comments.
- Code comments and docstrings.
- Repository and local-only documentation, including README files, development guides,
  agent instructions, design notes, implementation plans, and working notes, whether tracked by Git or not.

Apply this rule whenever creating or updating these materials.

## Branch names

Use `<type>/<short-description>` for branch names. Follow Conventional Commits
types such as `feat/`, `fix/`, `docs/`, `refactor/`, `test/`, and `chore/`, choosing
the type that matches the work. Keep the description lowercase and hyphen-separated,
for example `feat/personal-dictionary`. Do not use the `codex/` prefix.

## Commit messages

Use the Conventional Commits format:

```text
feat(foo): bar
```

Keep the type and scope lowercase, and write a concise imperative summary after
the colon.
