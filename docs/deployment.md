# Server deployment and API

Open Typeless needs both the Go Business Server and a separate ASR service. This guide is for the person operating those services. Desktop users only need the backend URL supplied by their operator; see the [README](../README.md) for setup.

## Run the Business Server locally

Install Go 1.25 to match CI, then run the following command from the repository root. The ASR service must already be reachable at `INFERENCE_URL`.

The Business Server in `cmd/server` converts client multipart uploads into HTTP requests to the ASR service's `/transcribe` endpoint. Start it with:

```sh
INFERENCE_URL=http://localhost:18080 go run ./cmd/server
```

## Run the Business Server in Docker

The server container includes only the Business Server. Deploy ASR separately, for example:

```sh
docker run --rm -p 8080:8080 \
  -e INFERENCE_URL=http://your-asr-host:18080 \
  ghcr.io/import-ai/open-typeless:main
```

`INFERENCE_URL` must point to an ASR address reachable from inside the container; `localhost` does not refer to the host machine. Build a local image with `docker build -t open-typeless .`.

Image publication and version tags are documented in the [development guide](development.md#ci-and-release-builds).

## Configure the desktop client

Provide users with the full API base URL, including `/api/v1`, such as `http://127.0.0.1:8080/api/v1` for a client on the same machine, or `https://example.com/api/v1` behind a reverse proxy. The client appends `/health` and `/recognitions` to this base URL.

## API behavior

`POST /api/v1/recognitions` accepts `audio` (WAV/MP3/M4A/WebM) and optional `language` and `hotwords` fields. Its response includes the PRD fields `raw_text`, `polished_text`, `language`, and `duration_ms`. No polishing model is integrated yet, so `polished_text` equals `raw_text`. `GET /api/v1/health` returns `{"status":"ok","version":"v0.1.0"}`. The debug server uses the same routes and also returns `audio_dir`.

The debug server is an optional local development tool; see [debug recording uploads](development.md#debug-recording-uploads).

Recognition is not streamed. The Business Server forwards the audio as the raw HTTP body to ASR `/transcribe` and maps dictionary `hotwords` to the `context` query parameter. The complete dictionary is limited to 1000 UTF-8 bytes, including newline separators.

ASR models run in the separate inference service, not in the desktop client or Business Server container. Experimental services and deployment notes under the local `deploy/asr/` directory are not tracked by Git and are not included in a fresh checkout.
