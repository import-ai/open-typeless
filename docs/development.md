# Development guide

This guide covers local development, debugging, implementation details, and release builds. For everyday use, see the [README](../README.md).

## Run the desktop client

The Tauri client lives in `tauri-client/`. Its React + TypeScript frontend is in `tauri-client/frontend/` and uses Vite, Tailwind CSS, and shadcn/ui. Install Node 22.12+, Rust, and the Tauri 2 system dependencies, then run:

```sh
cd tauri-client
npm install
npm run tauri dev
```

`tauri dev` starts Vite automatically, and frontend edits update through HMR. Other development commands:

```sh
npm run tauri:debug  # Persistent native previews of all four pill states
npm run dev         # Browser preview only; does not use the microphone
npm run typecheck   # Strict TypeScript checks
npm run build       # Type checks and dist/ generation
npm run tauri build # Desktop build; builds the frontend automatically
```

Add shadcn components by running `npx shadcn@latest add <component>` from `tauri-client/`. Component source lives in `frontend/src/components/ui/`. See the [frontend development guide](frontend.md) for integration rationale and directory conventions.

## Backend authentication

The Go Business Server requires `BACKEND_API_KEY`; startup fails if it is empty
or contains spaces, control characters, or non-ASCII characters. Generate a
random key (for example, `openssl rand -hex 32`) and supply it through a private
environment file or your service manager. With the key exported, run
`go run ./cmd/server` using the ASR/LLM configuration for your deployment.
`BACKEND_API_KEY` is separate from `LLM_API_KEY`, which authenticates Go to an
upstream LLM.

Both `/api/v1/health` and `/api/v1/recognitions` require
`Authorization: Bearer <key>`. Missing or incorrect keys return HTTP 401 before
reading uploaded audio or calling inference. Only CORS preflight is anonymous.
Keys in query parameters are not accepted. Use HTTPS for public deployments;
HTTP does not encrypt the key or recordings.

The desktop settings tab saves the API base URL and key together. Requests use
the saved key for health checks and uploads, and report 401/403 as an API key
error. A recording captures a URL/key snapshot; settings changes affect the
next recording. HTTP redirects are rejected. Input fields suspend the activation
shortcut while focused, and the key field masks its value. Browser previews
keep the key in memory only.

The desktop key is stored unencrypted in `~/.open-typeless/settings.json`
(or `%USERPROFILE%\.open-typeless\settings.json` on Windows), using the existing
atomic settings writes. Do not share this file. Older settings files load with
an empty key and retain their other preferences. The local debug server remains
unauthenticated and is intended only for local testing.

## Debug recording uploads

The optional `cmd/debug-server` tool is kept locally and is not included in a fresh Git checkout. If it is available in your workspace, use it as follows.

To debug client recording, temporarily run `cmd/debug-server` on port 8080. It does not call ASR: it saves uploaded audio to `/tmp/open-typeless-debug` and always returns `raw_text: "foo"`:

```sh
go run ./cmd/debug-server
ls -lh /tmp/open-typeless-debug
```

## Preview and validate the pill

Open `http://localhost:5173/?view=pill-debug` in a browser to work on the pill in isolation.

When started with `npm run tauri:debug`, the main window's developer options can show or hide the native pill and switch among disconnected, unready, ready, and processing states. Selecting a state immediately displays its appearance. This preview does not activate the microphone or call ASR, and is unavailable during recording or recognition. For native visual regression checks, test `processing -> disconnected` and `processing -> ready -> disconnected`; the capsule's rounded ends and bottom edge must remain intact. The capsule and recognition circle use separate elements with fixed dimensions and switch opacity, avoiding residual macOS clipping caused by resizing a shared element.

The pill initially appears centered horizontally on the main screen, with its bottom edge 128 logical pixels above the screen bottom. The waveform, right-side waiting indicator, and recognition circle can all be dragged and display a grab cursor. The dragged position persists until the application exits. On macOS, the pill accepts the first mouse click without taking keyboard focus. Esc cancels the current recording or recognition while the pill is visible and is released when the pill is hidden. Cancellation stops the client request and prevents results from old sessions from being pasted.

## Shortcut and recognition behavior

The macOS default shortcut is `RCommand`: press and release right Command by itself to start recording, then repeat to stop and recognize. Using another key, modifier, or mouse click while holding it cancels that shortcut activation, so combinations such as right Command+C do not start recording. Native AppKit local and global event listeners implement this behavior and require accessibility permission. The UI shows a prompt if permission is missing; restart the application after granting it. Windows defaults to standalone right Control (`RControl`), detected through a native keyboard listener. In settings, capture a single key or a conventional key combination; it takes effect on release. The shortcut, backend URL, and API key are saved automatically and restored at the next launch.

After recording stops, the client creates a temporary WAV and uploads it to the Business Server. It writes nonempty `polished_text` to the clipboard, falling back to `raw_text` when the field is absent, null, or blank, and simulates `Ctrl/Command+V` to paste into the current window. The server URL is empty by default. Enter a backend address in the main window's settings tab; pressing Enter or moving focus saves it automatically. Use `http://127.0.0.1:8080/api/v1` for a local service or, for example, `https://example.com/api/v1` behind a reverse proxy. The client treats this as the complete API base URL and appends only `/recognitions` for recognition requests.

Shortcut capture starts only when the input itself is clicked. Press the desired keys and release them to apply the shortcut without a save button. Ordinary single keys and standalone left/right Command, Ctrl, Shift, and Alt are supported, as are modifier-plus-key combinations. Click elsewhere to cancel capture. Esc can also be the activation key, but it still cancels recognition while the pill is visible. Recording is disabled until the backend URL is configured. Regular `tauri dev` does not show developer options.

Settings are restored at startup. An empty backend URL produces an unconfigured-backend status; otherwise the client requests `<base_url>/health` and reports ready only after a successful check. Health is checked again every 30 seconds while idle and before each recording.

## Settings and dictionary persistence

The client application identifier is `pro.omnibox.open-typeless`. Settings and the dictionary are stored in `.open-typeless` under the user's home directory, independently of that identifier. The settings path is `~/.open-typeless/settings.json` on macOS and `%USERPROFILE%\.open-typeless\settings.json` on Windows. Older versions used system application configuration directories: `~/Library/Application Support/com.opentypeless.client/` on macOS and `%APPDATA%\com.opentypeless.client\` on Windows. To preserve existing data, exit the application and copy `settings.json` and `dictionary.json` from the old directory into `.open-typeless`; migration is not automatic. Saving uses a temporary file and atomic replacement. Write failures display an error and preserve existing settings.

The main window's dictionary tab supports adding, editing, searching, individual deletion, and bulk deletion. New entries appear first, and edits retain the original position. Duplicate detection ignores English letter case. Select-all after a search selects only current results; changing the search clears the selection. Enter submits additions and edits, and Esc cancels them. Enter used to confirm a Chinese IME candidate does not submit the entry. Voice activation shortcuts are suspended while entering or searching dictionary terms to prevent accidental activation by single keys.

The dictionary is stored in `dictionary.json` alongside settings, using versioned JSON and atomic writes, and is restored on restart. Browser previews use separate localStorage and do not modify the desktop dictionary.

Each recording starts with a fixed dictionary snapshot. When recording stops, all terms are joined with newlines and uploaded as `hotwords`; the Go server forwards them to ASR as `context` (legacy) or `prompt` (audio.cpp). Dictionary edits take effect on the next recording. Dictionary terms are recognition hints whose effectiveness depends on the ASR model, without forced replacement. Optional LLM polishing runs after ASR; the client returns and pastes the same selected output text.

The complete dictionary is limited to **1000 UTF-8 bytes**, including separators, matching the Go API limit. Additions and edits are validated in advance; exceeding the limit produces an explicit error without silent truncation. Corrupt dictionary files produce an error and are preserved.

## Application icons

`tauri-client/icons/icon.svg` is the single source for the application icon, with separate background, waveform, and cat-head groups. `tauri dev`, `tauri build`, and GitHub desktop builds generate PNG, macOS ICNS, and Windows ICO files in the ignored `tauri-client/icons/generated/` directory. All packaging configuration uses these generated files. Run `npm run generate:icons` from `tauri-client/` to generate them separately; also do this before running Cargo builds or tests directly. Old PNGs, design variants, and historical exports are kept in the local `docs/archive/icons/` directory (not tracked by Git) and do not participate in builds.

## CI and release builds

GitHub Actions runs for pull requests, pushes to `main`, `v*` tag pushes, and manual triggers:

- **Desktop build** uses GitHub-hosted runners to build DMGs for macOS Apple Silicon / Intel and an NSIS EXE installer for Windows x64. Download installers directly from the run's **Artifacts**, without a ZIP wrapper. Developer ID signing, notarization, and Windows code signing are not configured yet.
- **Server build** runs Go tests and vet, then builds and pushes images for `linux/amd64` and `linux/arm64` to `ghcr.io/import-ai/open-typeless` using `GITHUB_TOKEN`. Pull requests produce a `:pr-<number>` image; the `main` branch produces `:main`; for example, the `v0.1.0` tag produces `:0.1.0` and `:0.1` (prereleases do not update `latest`). Manual runs use the selected branch or version tag.

Desktop installer names use the version from `tauri-client/tauri.conf.json` and the architecture names `arm64` or `amd64`:

- Builds on `v*` tags use `open-typeless-v<version>-<arch>.<extension>`.
- Other builds use `open-typeless-v<version>-<run_id><attempt_id>-<arch>.<extension>`, concatenating `GITHUB_RUN_ID` and `GITHUB_RUN_ATTEMPT` without a separator.
- macOS uses `.dmg`; Windows uses `.exe`. For example, release installers are `open-typeless-v0.1.0-arm64.dmg`, `open-typeless-v0.1.0-amd64.dmg`, and `open-typeless-v0.1.0-amd64.exe`.

Before releasing the client, synchronize the application version in `tauri-client/tauri.conf.json`, `tauri-client/Cargo.toml`, and `tauri-client/Cargo.lock`. Maintain the server health-check version in `internal/buildinfo/version.go`.

For validation commands and contribution conventions, see [AGENTS.md](../AGENTS.md).
