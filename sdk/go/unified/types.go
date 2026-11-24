package unified

import "fmt"

// UnsupportedFeatureError is returned when a capability has no lossless mapping.
type UnsupportedFeatureError struct {
	Protocol string
	Feature  string
}

func (e *UnsupportedFeatureError) Error() string {
	return fmt.Sprintf("unsupported feature %q for protocol %q", e.Feature, e.Protocol)
}

// ContentBlock represents a normalized LMP message content block.
type ContentBlock struct {
	Type       string         `json:"type"`
	Text       string         `json:"text,omitempty"`
	URL        string         `json:"url,omitempty"`
	MediaType  string         `json:"mediaType,omitempty"`
	Data       string         `json:"data,omitempty"`
	Thinking   string         `json:"thinking,omitempty"`
	Signature  string         `json:"signature,omitempty"`
	ID         string         `json:"id,omitempty"`
	Name       string         `json:"name,omitempty"`
	Arguments  map[string]any `json:"arguments,omitempty"`
	ToolCallID string         `json:"toolCallId,omitempty"`
	Content    string         `json:"content,omitempty"`
	IsError    bool           `json:"isError,omitempty"`
}

// Message represents a normalized conversation turn.
type Message struct {
	Role    string         `json:"role"`
	Content []ContentBlock `json:"content"`
}

// ThinkingConfig configures reasoning effort or token budget.
type ThinkingConfig struct {
	Effort       string `json:"effort,omitempty"`
	BudgetTokens int64  `json:"budgetTokens,omitempty"`
}

// ResponseFormat configures structured output.
type ResponseFormat struct {
	Type   string         `json:"type"`
	Name   string         `json:"name,omitempty"`
	Schema map[string]any `json:"schema,omitempty"`
	Strict *bool          `json:"strict,omitempty"`
}

// Request is an LMP completion request.
type Request struct {
	Model              string          `json:"model"`
	Messages           []Message       `json:"messages"`
	MaxTokens          int64           `json:"maxTokens,omitempty"`
	Temperature        *float64        `json:"temperature,omitempty"`
	Thinking           *ThinkingConfig `json:"thinking,omitempty"`
	ResponseFormat     *ResponseFormat `json:"responseFormat,omitempty"`
	ProviderExtensions map[string]any  `json:"providerExtensions,omitempty"`
}

// Usage reports normalized token and prompt cache metrics.
type Usage struct {
	InputTokens      int64  `json:"inputTokens"`
	OutputTokens     int64  `json:"outputTokens"`
	TotalTokens      int64  `json:"totalTokens"`
	ReasoningTokens  *int64 `json:"reasoningTokens,omitempty"`
	CacheReadTokens  *int64 `json:"cacheReadTokens,omitempty"`
	CacheWriteTokens *int64 `json:"cacheWriteTokens,omitempty"`
}

// Response is a normalized LMP completion response.
type Response struct {
	ID           string         `json:"id,omitempty"`
	Model        string         `json:"model,omitempty"`
	Content      []ContentBlock `json:"content"`
	FinishReason string         `json:"finishReason,omitempty"`
	Usage        Usage          `json:"usage"`
	Raw          map[string]any `json:"raw"`
}

func (r *Response) Text() string {
	var out string
	for _, b := range r.Content {
		if b.Type == "text" {
			out += b.Text
		}
	}
	return out
}

func (r *Response) ThinkingText() string {
	var out string
	for _, b := range r.Content {
		if b.Type == "thinking" {
			out += b.Thinking
		}
	}
	return out
}

// StreamEvent represents a normalized streaming delta or lifecycle event.
type StreamEvent struct {
	Type         string
	ID           string
	Delta        string
	FinishReason string
	Usage        *Usage
}
