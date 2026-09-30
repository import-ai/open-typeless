package main

import (
	"bytes"
	"context"
	_ "embed"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strings"
	"time"
)

//go:embed polish_messages.json
var polishMessagesJSON []byte

type chatMessage struct {
	Role    string `json:"role"`
	Content string `json:"content"`
}

type polishRequest struct {
	Model              string        `json:"model"`
	Messages           []chatMessage `json:"messages"`
	MaxTokens          int           `json:"max_tokens"`
	Stream             bool          `json:"stream"`
	ChatTemplateKwargs struct {
		EnableThinking bool `json:"enable_thinking"`
	} `json:"chat_template_kwargs"`
}

type polisher struct {
	baseURL  string
	model    string
	apiKey   string
	timeout  time.Duration
	messages []chatMessage
	client   *http.Client
}

func polisherFromEnv() (*polisher, error) {
	base := strings.TrimRight(strings.TrimSpace(os.Getenv("LLM_BASE_URL")), "/")
	if base == "" {
		return nil, nil
	}
	u, err := url.Parse(base)
	if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" {
		return nil, errors.New("LLM_BASE_URL must be an HTTP(S) API base URL without credentials, query, or fragment")
	}
	timeout, err := time.ParseDuration(env("LLM_TIMEOUT", "10s"))
	if err != nil || timeout <= 0 {
		return nil, errors.New("LLM_TIMEOUT must be a positive duration, such as 10s")
	}
	messages, err := loadPolishMessages()
	if err != nil {
		return nil, err
	}
	return &polisher{
		baseURL: base, model: env("LLM_MODEL", "minicpm5-2b-q4"),
		apiKey: os.Getenv("LLM_API_KEY"), timeout: timeout, messages: messages,
		client: &http.Client{CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }},
	}, nil
}

func loadPolishMessages() ([]chatMessage, error) {
	data := polishMessagesJSON
	path := strings.TrimSpace(os.Getenv("LLM_PROMPT_FILE"))
	if path != "" {
		var err error
		data, err = os.ReadFile(path)
		if err != nil {
			return nil, fmt.Errorf("failed to read LLM_PROMPT_FILE: %w", err)
		}
	}
	var messages []chatMessage
	if err := json.Unmarshal(data, &messages); err != nil {
		return nil, fmt.Errorf("invalid polishing prompt: %w", err)
	}
	return messages, nil
}

func (p *polisher) polish(parent context.Context, raw string) (string, error) {
	ctx, cancel := context.WithTimeout(parent, p.timeout)
	defer cancel()
	messages := p.messages
	if messages == nil {
		var err error
		messages, err = loadPolishMessages()
		if err != nil {
			return "", err
		}
	}
	messages = append([]chatMessage(nil), messages...)
	messages = append(messages, chatMessage{Role: "user", Content: raw})
	body, err := json.Marshal(polishRequest{Model: p.model, Messages: messages, MaxTokens: 512})
	if err != nil {
		return "", errors.New("failed to encode polishing request")
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, p.baseURL+"/chat/completions", bytes.NewReader(body))
	if err != nil {
		return "", errors.New("failed to build polishing request")
	}
	req.Header.Set("Content-Type", "application/json")
	if p.apiKey != "" {
		req.Header.Set("Authorization", "Bearer "+p.apiKey)
	}
	resp, err := p.client.Do(req)
	if err != nil {
		// Do not log upstream bodies, URLs, credentials, or transcript text.
		if ctx.Err() != nil {
			return "", ctx.Err()
		}
		return "", errors.New("LLM unavailable")
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("LLM returned HTTP %d", resp.StatusCode)
	}
	const maxResponseBytes = 2 << 20
	data, err := io.ReadAll(io.LimitReader(resp.Body, maxResponseBytes+1))
	if err != nil || len(data) > maxResponseBytes {
		return "", errors.New("failed to read LLM response")
	}
	var result struct {
		Choices []struct {
			Message struct {
				Content   string            `json:"content"`
				Refusal   string            `json:"refusal"`
				ToolCalls []json.RawMessage `json:"tool_calls"`
			} `json:"message"`
			FinishReason string `json:"finish_reason"`
		} `json:"choices"`
	}
	if err := json.Unmarshal(data, &result); err != nil || len(result.Choices) != 1 {
		return "", errors.New("invalid LLM response")
	}
	choice := result.Choices[0]
	text := strings.TrimSpace(choice.Message.Content)
	if choice.FinishReason != "stop" || text == "" || choice.Message.Refusal != "" || len(choice.Message.ToolCalls) != 0 {
		return "", errors.New("LLM did not return a complete text result")
	}
	return text, nil
}
