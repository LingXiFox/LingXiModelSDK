package native

import (
	"bytes"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"testing/iotest"
)

func TestAmazonEventStreamFrames(t *testing.T) {
	fixture := func(name string) []byte {
		data, err := os.ReadFile(filepath.Join("..", "..", "..", "conformance", "native", name))
		if err != nil {
			t.Fatal(err)
		}
		return data
	}
	data := fixture("aws_bedrock_stream.eventstream")
	var events []SSEEvent
	err := readAmazonEvents(iotest.OneByteReader(bytes.NewReader(data)), func(ev SSEEvent) error {
		events = append(events, ev)
		return nil
	})
	if err != nil || len(events) != 8 || !strings.Contains(events[4].Data, "東京") {
		t.Fatalf("fragmented events = %+v, error = %v", events, err)
	}
	emit := func(SSEEvent) error { return nil }
	if err := readAmazonEvents(bytes.NewReader(data[:len(data)-1]), emit); !errors.Is(err, io.ErrUnexpectedEOF) {
		t.Fatalf("expected truncation, got %v", err)
	}
	data[len(data)-1] ^= 1
	if err := readAmazonEvents(bytes.NewReader(data), emit); err == nil || !strings.Contains(err.Error(), "checksum") {
		t.Fatalf("expected checksum error, got %v", err)
	}
	if err := readAmazonEvents(bytes.NewReader(fixture("aws_bedrock_error.eventstream")), emit); err == nil || !strings.Contains(err.Error(), "error frame") {
		t.Fatalf("expected upstream error, got %v", err)
	}
}
