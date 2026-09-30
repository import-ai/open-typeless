package main

import (
	"bytes"
	"context"
	"mime/multipart"
	"net/http"
	"net/url"
)

func (s *server) inferenceRequest(ctx context.Context, audio []byte, filename, language, hotwords string) (*http.Request, error) {
	if s.inferenceProtocol != "audiocpp" {
		query := url.Values{"language": {language}}
		if hotwords != "" {
			query.Set("context", hotwords)
		}
		req, err := http.NewRequestWithContext(ctx, http.MethodPost, s.inferenceURL+"/transcribe?"+query.Encode(), bytes.NewReader(audio))
		if err == nil {
			req.Header.Set("Content-Type", "application/octet-stream")
		}
		return req, err
	}
	var body bytes.Buffer
	form := multipart.NewWriter(&body)
	part, err := form.CreateFormFile("file", filename)
	if err != nil {
		return nil, err
	}
	if _, err = part.Write(audio); err != nil {
		return nil, err
	}
	fields := map[string]string{"model": s.inferenceModel}
	// audio.cpp rejects the literal language "auto"; omission enables detection.
	if language != "auto" && language != "" {
		fields["language"] = language
	}
	if hotwords != "" {
		fields["prompt"] = hotwords
	}
	for key, value := range fields {
		if err := form.WriteField(key, value); err != nil {
			return nil, err
		}
	}
	if err := form.Close(); err != nil {
		return nil, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, s.inferenceURL+"/v1/audio/transcriptions", &body)
	if err == nil {
		req.Header.Set("Content-Type", form.FormDataContentType())
	}
	return req, err
}
