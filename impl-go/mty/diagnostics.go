// Package mty implements an independent (third) Mighty front-end in pure Go.
//
// Spec reference: docs/spec/v1.0-rc.md (RC3). No peeking at Rust or Python
// implementations was used during construction; spec-only derivation.
package mty

import "fmt"

// Severity ranks diagnostics.
type Severity int

const (
	SevError Severity = iota
	SevWarning
	SevInfo
)

func (s Severity) String() string {
	switch s {
	case SevError:
		return "error"
	case SevWarning:
		return "warning"
	default:
		return "info"
	}
}

// Span is a half-open byte range in the source file.
type Span struct {
	Start int // inclusive byte offset
	End   int // exclusive byte offset
}

// Diagnostic carries a code (e.g. MT0001), severity, message, and span.
//
// Codes per v1.0-RC §33 banding:
//
//	MT0xxx — lexer / generic IO
//	MT1xxx — parser / CST
type Diagnostic struct {
	Code     string
	Severity Severity
	Message  string
	Span     Span
}

func (d Diagnostic) String() string {
	return fmt.Sprintf("%s[%s] %s (at %d..%d)", d.Severity, d.Code, d.Message, d.Span.Start, d.Span.End)
}

// LineCol resolves a byte offset to a 1-based (line, col) pair within src.
func LineCol(src string, offset int) (line, col int) {
	if offset > len(src) {
		offset = len(src)
	}
	line = 1
	col = 1
	for i := 0; i < offset; i++ {
		if src[i] == '\n' {
			line++
			col = 1
		} else {
			col++
		}
	}
	return line, col
}
