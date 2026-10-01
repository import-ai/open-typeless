# Open Typeless

Speak, then insert the recognized text into the app you are using. Open Typeless is a desktop voice input tool for macOS and Windows.

- Start and finish recording with a keyboard shortcut.
- Add frequently used terms to your personal dictionary to help recognition.
- Move the floating recording indicator or press Esc to cancel.
- Review local recording history and see your dictation activity and estimated time saved.

Recognition happens after you finish speaking. Open Typeless requires a configured recognition service; ask the person running it for a backend URL and API key. Recordings and dictionary terms are sent to that service for recognition.

## Install

Download the installer for your computer from a successful [Desktop build](https://github.com/import-ai/open-typeless/actions/workflows/build.yml) run under **Artifacts**:

| Computer | Download | Installer |
| --- | --- | --- |
| Mac with Apple Silicon | `open-typeless-v<version>-arm64.dmg` | DMG |
| Mac with an Intel processor | `open-typeless-v<version>-amd64.dmg` | DMG |
| Windows x64 PC | `open-typeless-v<version>-amd64.exe` | EXE |

Non-release builds include the workflow run ID immediately followed by its attempt number, for example `open-typeless-v0.1.0-1234561-arm64.dmg` for run `123456`, attempt `1`. Download and open the installer directly to install Open Typeless. Current builds are unsigned, so your operating system may show a security prompt.

## Set up

1. Open Open Typeless and go to the settings tab.
2. Enter the backend URL supplied by your service operator, for example `https://example.com/api/v1`. Press Enter or click elsewhere to save it.
3. Enter the API key supplied by your service operator. Press Enter or click elsewhere to save it.
4. On macOS, allow microphone access and grant accessibility permission in **System Settings > Privacy & Security**. Restart the app after granting accessibility permission.
5. Wait for the app to report that it is ready.

Your backend address, API key, and shortcut are saved automatically. Recording is unavailable until the backend is configured, reachable, and accepts your API key.

## Start speaking

1. Click where you want to insert text in another app.
2. Press and release **right Command** on macOS or **right Control** on Windows, by itself.
3. Speak, then press and release the same key again to finish.
4. Wait for recognition to complete. The text is pasted into the app.

The app checks whether your backend supports streaming ASR and uses it automatically when available, sending audio while you speak. Otherwise, it uploads the completed recording as before. If a stream fails, the app retries with the complete recording. Text is polished and pasted once, after you stop recording. Recording stops automatically at the service’s audio size limit (up to 12 MiB) or four minutes, whichever comes first, and shows a warning.

Press **Esc** while recording or waiting for recognition to cancel. You can drag the floating recording indicator to a convenient position; it stays there until you quit the app.

If recording or recognition fails, the floating indicator turns red and shows an error. If a recording was saved but recognition or history storage failed, the message includes its location for manual recovery. Hover over it for details, then press **Esc** or click its close button to dismiss it. Check the backend settings if it reports a missing or unavailable backend.

To change the shortcut, click the shortcut field in settings, press your preferred key or key combination, then release it to save. Click elsewhere to cancel shortcut capture.

## Personal dictionary

Use the dictionary tab to add terms you say often, such as product names or technical vocabulary. You can edit, search, and delete entries individually or in bulk. Changes apply to your next recording and are saved for future sessions.

Dictionary terms help guide recognition, but do not guarantee an exact match. If the dictionary reaches its capacity, the app will ask you to shorten or remove entries.

## History and insights

The history tab keeps successful, nonempty transcriptions and their recordings on your computer. Expand a row to read the full result, or use its menu to locate the recording in Finder / File Explorer, copy the raw or polished text, or delete the entry. Deletion also removes its recording and cannot be undone through the app. If no polished result was returned, the row displays the raw text and copying a polished result is unavailable.

The voice input page shows lifetime character count, audio duration, dictation speed, estimated time saved, and activity for the current month plus the previous five months. Characters are counted from the raw transcript: Chinese characters, letters, and digits count; spaces, punctuation, and emoji do not. Audio duration measures the uploaded WAV after leading silence is trimmed. Time saved assumes typing at **30 characters per minute** and can be negative. Cancelled, failed, and empty recognitions do not count. A paste failure does not remove a completed transcription.

Deleting history does not reduce the lifetime statistics or calendar activity. History, recordings, and daily totals stay in `~/.open-typeless/` on macOS or `%USERPROFILE%\.open-typeless\` on Windows, with no history synchronization. Audio is still sent to your configured recognition service as described above. Collection starts when you first use a version with this feature; earlier usage cannot be recovered.

## Need help?

- **The app is not ready:** check the backend address and API key, and confirm with your service operator that the service is running.
- **Recording does not start:** check microphone access. On macOS, also check accessibility permission and restart the app after changing it.
- **The shortcut does not trigger:** press and release the default modifier key on its own, without another key or mouse click. You can also choose a different shortcut in settings.

## For developers and service operators

- [Development, debugging, and builds](docs/development.md)
- [Frontend development](docs/frontend.md)
