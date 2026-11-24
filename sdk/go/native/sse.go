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

// ReadSSEEach parses an SSE stream incrementally and invokes fn for each event as it arrives.
func ReadSSEEach(r io.Reader, fn func(SSEEvent) error) error {
	scanner := bufio.NewScanner(r)
	scanner.Buffer(make([]byte, 0, 64*1024), 4*1024*1024)

	var eventName string
	var dataLines []string
	var lastID string

	flush := func() error {
		if len(dataLines) == 0 && eventName == "" {
			return nil
		}
		ev := SSEEvent{
			Event: eventName,
			Data:  strings.Join(dataLines, "\n"),
			ID:    lastID,
		}
		eventName = ""
		dataLines = nil
		return fn(ev)
	}

	for scanner.Scan() {
		line := strings.TrimSuffix(scanner.Text(), "\r")
		if line == "" {
			if err := flush(); err != nil {
				return err
			}
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
		return err
	}
	return flush()
}

// ReadNDJSONEach parses an NDJSON stream incrementally and invokes fn for each JSON line.
func ReadNDJSONEach(r io.Reader, fn func(SSEEvent) error) error {
	scanner := bufio.NewScanner(r)
	scanner.Buffer(make([]byte, 0, 64*1024), 4*1024*1024)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}
		if err := fn(SSEEvent{Data: line}); err != nil {
			return err
		}
	}
	return scanner.Err()
}

// ReadSSE parses an SSE stream incrementally until EOF.
func ReadSSE(r io.Reader) ([]SSEEvent, error) {
	var events []SSEEvent
	err := ReadSSEEach(r, func(ev SSEEvent) error {
		events = append(events, ev)
		return nil
	})
	return events, err
}
