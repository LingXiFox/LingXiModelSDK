package native

import (
	"encoding/binary"
	"encoding/json"
	"fmt"
	"hash/crc32"
	"io"
	"unicode/utf8"
)

const amazonEventStream = "application/vnd.amazon.eventstream"

func readAmazonEvents(r io.Reader, emit func(SSEEvent) error) error {
	fail := func(reason string) error { return fmt.Errorf("Amazon Event Stream: %s", reason) }
	var prelude [12]byte
	for {
		if _, err := io.ReadFull(r, prelude[:]); err != nil {
			if err == io.EOF {
				return nil
			}
			return fmt.Errorf("Amazon Event Stream prelude: %w", err)
		}
		total, headers := binary.BigEndian.Uint32(prelude[:4]), binary.BigEndian.Uint32(prelude[4:8])
		if crc32.ChecksumIEEE(prelude[:8]) != binary.BigEndian.Uint32(prelude[8:]) {
			return fail("invalid prelude checksum")
		}
		if headers > 128<<10 || total < 16+headers || total-16-headers > 16<<20 {
			return fail("invalid frame size")
		}
		data := make([]byte, total-12)
		if _, err := io.ReadFull(r, data); err != nil {
			return fmt.Errorf("Amazon Event Stream frame: %w", err)
		}
		frame := data[:len(data)-4]
		checksum := crc32.Update(crc32.ChecksumIEEE(prelude[:]), crc32.IEEETable, frame)
		if checksum != binary.BigEndian.Uint32(data[len(frame):]) {
			return fail("invalid message checksum")
		}
		kind, name, err := amazonEventHeaders(frame[:headers])
		if err != nil {
			return err
		}
		if kind == "error" || kind == "exception" {
			return fail("upstream sent an error frame")
		}
		if kind != "event" || name == "" {
			return fail("missing or invalid event header")
		}
		payload := frame[headers:]
		if !utf8.Valid(payload) || !json.Valid(payload) {
			return fail("invalid event JSON")
		}
		key, _ := json.Marshal(name)
		if err := emit(SSEEvent{Event: name, Data: "{" + string(key) + ":" + string(payload) + "}"}); err != nil {
			return err
		}
	}
}

func amazonEventHeaders(data []byte) (kind, name string, err error) {
	invalid := fmt.Errorf("Amazon Event Stream: invalid header")
	for len(data) > 0 {
		keyLen := int(data[0])
		if len(data) < keyLen+2 {
			return "", "", invalid
		}
		key, tag := string(data[1:1+keyLen]), data[1+keyLen]
		data = data[2+keyLen:]
		var size int
		switch tag {
		case 0, 1:
		case 2:
			size = 1
		case 3:
			size = 2
		case 4:
			size = 4
		case 5, 8:
			size = 8
		case 9:
			size = 16
		case 6, 7:
			if len(data) < 2 {
				return "", "", invalid
			}
			size = int(binary.BigEndian.Uint16(data))
			data = data[2:]
		default:
			return "", "", invalid
		}
		if len(data) < size {
			return "", "", invalid
		}
		value := data[:size]
		data = data[size:]
		var target *string
		switch key {
		case ":message-type":
			target = &kind
		case ":event-type":
			target = &name
		default:
			continue
		}
		if tag != 7 || *target != "" || len(value) == 0 || !utf8.Valid(value) {
			return "", "", invalid
		}
		*target = string(value)
	}
	return kind, name, nil
}
