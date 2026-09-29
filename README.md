# Open Typeless

Speak, then insert the recognized text into the app you are using. Open Typeless is a desktop voice input tool for macOS and Windows.

- Start and finish recording with a keyboard shortcut.
- Add frequently used terms to your personal dictionary to help recognition.
- Move the floating recording indicator or press Esc to cancel.

Recognition happens after you finish speaking. Open Typeless requires a configured recognition service; ask the person running it for a backend URL, or follow the [server setup guide](docs/deployment.md) to host your own. Recordings and dictionary terms are sent to that service for recognition.

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
3. On macOS, allow microphone access and grant accessibility permission in **System Settings > Privacy & Security**. Restart the app after granting accessibility permission.
4. Wait for the app to report that it is ready.

Your backend address and shortcut are saved automatically. Recording is unavailable until the backend is configured and reachable.

## Start speaking

1. Click where you want to insert text in another app.
2. Press and release **right Command** on macOS or **right Control** on Windows, by itself.
3. Speak, then press and release the same key again to finish.
4. Wait for recognition to complete. The text is pasted into the app.

Press **Esc** while recording or waiting for recognition to cancel. You can drag the floating recording indicator to a convenient position; it stays there until you quit the app.

To change the shortcut, click the shortcut field in settings, press your preferred key or key combination, then release it to save. Click elsewhere to cancel shortcut capture.

## Personal dictionary

Use the dictionary tab to add terms you say often, such as product names or technical vocabulary. You can edit, search, and delete entries individually or in bulk. Changes apply to your next recording and are saved for future sessions.

Dictionary terms help guide recognition, but do not guarantee an exact match. If the dictionary reaches its capacity, the app will ask you to shorten or remove entries.

## Need help?

- **The app is not ready:** check the backend address and confirm with your service operator that the service is running.
- **Recording does not start:** check microphone access. On macOS, also check accessibility permission and restart the app after changing it.
- **The shortcut does not trigger:** press and release the default modifier key on its own, without another key or mouse click. You can also choose a different shortcut in settings.

## For developers and service operators

- [Development, debugging, and builds](docs/development.md)
- [Server deployment and API](docs/deployment.md)
- [Frontend development](docs/frontend.md)
