package native

import (
	"bufio"
	"io"
	"strings"
)

// SSEEvent represents a single Server-Sent Event frame.
type SSEEvent struct {
	Event string
	Data  string
	ID    string
}

// ReadSSE parses an SSE stream incrementally until EOF.
func ReadSSE(r io.Reader) ([]SSEEvent, error) {
	scanner := bufio.NewScanner(r)
	scanner.Buffer(make([]byte, 0, 64*1024), 4*1024*1024)

	var events []SSEEvent
	var eventName string
	var dataLines []string
	var lastID string

	flush := func() {
		if len(dataLines) == 0 && eventName == "" {
			return
		}
		events = append(events, SSEEvent{
			Event: eventName,
			Data:  strings.Join(dataLines, "\n"),
			ID:    lastID,
		})
		eventName = ""
		dataLines = nil
	}

	for scanner.Scan() {
		line := strings.TrimSuffix(scanner.Text(), "\r")
		if line == "" {
			flush()
			continue
		}
		if strings.HasPrefix(line, ":") {
			continue
		}
		field, val, found := strings.Cut(line, ":")
		if found {
			val = strings.TrimPrefix(val, " ")
		}
		switch field {
		case "event":
			eventName = val
		case "data":
			dataLines = append(dataLines, val)
		case "id":
			lastID = val
		}
	}
	if err := scanner.Err(); err != nil {
		return events, err
	}
	flush()
	return events, nil
}
