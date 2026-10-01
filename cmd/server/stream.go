package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
)

// A JSON line describes the following interleaved s16le PCM request body.
// Responses are NDJSON; only a final event is eligible for pasting.
type streamConfig struct {
	SampleRate int    `json:"sample_rate"`
	Channels   int    `json:"channels"`
	Language   string `json:"language"`
	Hotwords   string `json:"hotwords"`
}

type streamEvent struct {
	Type   string               `json:"type"`
	Text   string               `json:"text,omitempty"`
	Result *recognitionResponse `json:"result,omitempty"`
}

func (s *server) recognizeStream(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	if s.inferenceStreamModel == "" {
		writeError(w, http.StatusNotImplemented, "streaming ASR is not configured")
		return
	}
	rc := http.NewResponseController(w)
	if r.ProtoMajor == 1 {
		// Early rejection can leave unread PCM; never reuse that HTTP/1 connection.
		w.Header().Set("Connection", "close")
		if err := rc.EnableFullDuplex(); err != nil {
			writeError(w, http.StatusNotImplemented, "full duplex HTTP is unavailable")
			return
		}
	}
	// Bound both silent uploads and the total lifetime, including inference.
	ctx, cancel := context.WithTimeout(r.Context(), 5*time.Minute)
	defer cancel()
	_ = rc.SetReadDeadline(time.Now().Add(15 * time.Second))
	reader := bufio.NewReaderSize(r.Body, 4096)
	line, err := reader.ReadSlice('\n')
	var config streamConfig
	if err != nil || json.Unmarshal(line, &config) != nil ||
		config.SampleRate < 8000 || config.SampleRate > 192000 ||
		config.Channels < 1 || config.Channels > 8 || len(config.Hotwords) > 1000 || len(config.Language) > 64 {
		writeError(w, http.StatusBadRequest, "invalid streaming audio configuration")
		return
	}
	query := url.Values{
		"model": {s.inferenceStreamModel}, "sample_format": {"s16le"},
		"sample_rate": {strconv.Itoa(config.SampleRate)}, "channels": {strconv.Itoa(config.Channels)},
	}
	if config.Language != "" && config.Language != "auto" {
		query.Set("language", config.Language)
	}
	if config.Hotwords != "" {
		query.Set("prompt", config.Hotwords)
	}
	input, output := io.Pipe()
	uploaded := make(chan struct{})
	// Transport cancellation must also unblock a stalled downstream upload.
	interrupted := make(chan struct{})
	stopClose := context.AfterFunc(ctx, func() {
		_ = rc.SetReadDeadline(time.Now())
		_ = input.CloseWithError(ctx.Err())
		close(interrupted)
	})
	defer func() {
		cancel()
		if !stopClose() {
			<-interrupted
		}
		_ = rc.SetReadDeadline(time.Now())
		_ = input.Close()
		_ = r.Body.Close()
		<-uploaded // No request reader may outlive its ResponseWriter.
	}()
	var audioBytes int64
	var uploadErr error
	go func() {
		buffer := make([]byte, 32<<10)
		for {
			_ = rc.SetReadDeadline(time.Now().Add(15 * time.Second))
			n, readErr := reader.Read(buffer)
			audioBytes += int64(n)
			if audioBytes > s.maxBytes {
				uploadErr = errors.New("audio is too large")
				break
			}
			if n > 0 {
				if _, uploadErr = output.Write(buffer[:n]); uploadErr != nil {
					break
				}
			}
			if readErr != nil {
				if readErr != io.EOF {
					uploadErr = readErr
				}
				break
			}
		}
		if uploadErr == nil && (audioBytes == 0 || audioBytes%int64(2*config.Channels) != 0) {
			uploadErr = errors.New("empty or incomplete PCM audio")
		}
		close(uploaded)
		_ = output.CloseWithError(uploadErr)
	}()
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, s.inferenceURL+"/v1/audio/transcriptions/live?"+query.Encode(), input)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "invalid inference URL")
		return
	}
	req.Header.Set("Content-Type", "application/octet-stream")
	client := *s.client
	client.Timeout = 0 // Recording time is separate from the offline request budget.
	client.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	w.Header().Set("Content-Type", "application/x-ndjson")
	w.Header().Set("Cache-Control", "no-store")
	w.Header().Set("X-Accel-Buffering", "no")
	emit := func(event streamEvent) error {
		_ = rc.SetWriteDeadline(time.Now().Add(5 * time.Second))
		if err := json.NewEncoder(w).Encode(event); err != nil {
			return err
		}
		return rc.Flush()
	}
	if emit(streamEvent{Type: "ready"}) != nil {
		return
	}
	// Start the post-upload ASR budget even if the upstream never sends headers.
	go func() {
		select {
		case <-uploaded:
			if uploadErr != nil {
				cancel()
				return
			}
			timer := time.NewTimer(40 * time.Second)
			defer timer.Stop()
			select {
			case <-timer.C:
				cancel()
			case <-ctx.Done():
			}
		case <-ctx.Done():
		}
	}()
	resp, err := client.Do(req)
	if err != nil {
		_ = emit(streamEvent{Type: "error", Text: "streaming inference unavailable"})
		return
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK || !strings.HasPrefix(resp.Header.Get("Content-Type"), "text/event-stream") {
		_ = emit(streamEvent{Type: "error", Text: fmt.Sprintf("streaming inference rejected (HTTP %d)", resp.StatusCode)})
		return
	}
	raw, err := readTranscriptEvents(resp.Body, func(delta string) error { return emit(streamEvent{Type: "partial", Text: delta}) })
	if err == nil {
		// Never accept an upstream result that ignored an unfinished upload.
		select {
		case <-uploaded:
			err = uploadErr
		default:
			err = errors.New("inference ended before audio upload")
		}
	}
	if err != nil {
		_ = emit(streamEvent{Type: "error", Text: "streaming inference did not complete"})
		return
	}
	result := s.polishRecognition(r.Context(), raw, config.Language, int(audioBytes*1000/int64(2*config.Channels*config.SampleRate)))
	if r.Context().Err() == nil {
		_ = emit(streamEvent{Type: "final", Result: &result})
	}
}

// Read bounded SSE records, allowing comments, CRLF and multiline data fields.
func readTranscriptEvents(body io.Reader, partial func(string) error) (string, error) {
	scanner := bufio.NewScanner(io.LimitReader(body, (2<<20)+1))
	scanner.Buffer(make([]byte, 4096), 256<<10)
	var data string
	for scanner.Scan() {
		line := scanner.Text()
		if strings.HasPrefix(line, "data:") {
			data += strings.TrimPrefix(strings.TrimPrefix(line, "data:"), " ") + "\n"
			if len(data) > 256<<10 {
				return "", errors.New("SSE event too large")
			}
		} else if line == "" && data != "" {
			var event struct {
				Type  string  `json:"type"`
				Delta string  `json:"delta"`
				Text  *string `json:"text"`
			}
			if json.Unmarshal([]byte(data), &event) != nil {
				return "", errors.New("invalid SSE event")
			}
			data = ""
			switch event.Type {
			case "transcript.text.delta":
				if err := partial(event.Delta); err != nil {
					return "", err
				}
			case "transcript.text.done":
				if event.Text == nil {
					return "", errors.New("missing final transcript")
				}
				return *event.Text, nil
			case "error":
				return "", errors.New("inference stream failed")
			}
		}
	}
	return "", errors.New("inference stream ended without a final transcript")
}
