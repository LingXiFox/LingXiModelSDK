package unified

import (
	"encoding/json"
	"fmt"
)

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

// ToolChoice mirrors the LMP toolChoice union: one of the strings auto,
// none, required, or an object {"tool": "<name>"}.
type ToolChoice struct {
	Kind string
	Name string
}

// UnmarshalJSON accepts both wire shapes the LMP specification allows.
func (t *ToolChoice) UnmarshalJSON(data []byte) error {
	var name string
	if err := json.Unmarshal(data, &name); err == nil {
		switch name {
		case "auto", "none", "required":
			t.Kind, t.Name = name, ""
			return nil
		}
		return fmt.Errorf("unsupported toolChoice %q", name)
	}
	var obj struct {
		Tool string `json:"tool"`
	}
	if err := json.Unmarshal(data, &obj); err != nil || obj.Tool == "" {
		return fmt.Errorf("invalid toolChoice")
	}
	t.Kind, t.Name = "tool", obj.Tool
	return nil
}

// MarshalJSON reproduces the same two wire shapes.
func (t ToolChoice) MarshalJSON() ([]byte, error) {
	if t.Kind == "tool" {
		return json.Marshal(map[string]string{"tool": t.Name})
	}
	return json.Marshal(t.Kind)
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

// ToolDefinition describes a callable tool in LMP.
type ToolDefinition struct {
	Name        string         `json:"name"`
	Description string         `json:"description,omitempty"`
	Parameters  map[string]any `json:"parameters"`
}

// Request is an LMP completion request.
type Request struct {
	Model              string           `json:"model"`
	Messages           []Message        `json:"messages"`
	MaxTokens          int64            `json:"maxTokens,omitempty"`
	Temperature        *float64         `json:"temperature,omitempty"`
	Tools              []ToolDefinition `json:"tools,omitempty"`
	ToolChoice         *ToolChoice      `json:"toolChoice,omitempty"`
	Thinking           *ThinkingConfig  `json:"thinking,omitempty"`
	ResponseFormat     *ResponseFormat  `json:"responseFormat,omitempty"`
	ProviderExtensions map[string]any   `json:"providerExtensions,omitempty"`
}

type EmbeddingRequest struct {
	Model string   `json:"model"`
	Input []string `json:"input"`
}

type EmbeddingResponse struct {
	Embeddings [][]float64    `json:"embeddings"`
	Usage      Usage          `json:"usage"`
	Raw        map[string]any `json:"raw"`
}

type RerankRequest struct {
	Model     string   `json:"model"`
	Query     string   `json:"query"`
	Documents []string `json:"documents"`
	TopN      int      `json:"topN,omitempty"`
}

type RerankResult struct {
	Index          int     `json:"index"`
	RelevanceScore float64 `json:"relevanceScore"`
}

type RerankResponse struct {
	Results []RerankResult `json:"results"`
	Raw     map[string]any `json:"raw"`
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

// StreamEvent represents a normalized LMP streaming delta or lifecycle event.
//
// Field names follow the LMP streaming contract: the discriminating field is
// `type` in snake_case, every other field is camelCase.
type StreamEvent struct {
	Type           string `json:"type"`
	ID             string `json:"id,omitempty"`
	Delta          string `json:"delta,omitempty"`
	Signature      string `json:"signature,omitempty"`
	FinishReason   string `json:"finishReason,omitempty"`
	Usage          *Usage `json:"usage,omitempty"`
	Index          *int   `json:"index,omitempty"`
	Name           string `json:"name,omitempty"`
	ArgumentsDelta string `json:"argumentsDelta,omitempty"`
	Code           string `json:"code,omitempty"`
	Message        string `json:"message,omitempty"`
}
