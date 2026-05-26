package mty

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// findExamples walks up from the current package directory to find
// the repository-root examples/ directory (impl-go is a sibling of it).
func findExamples(t *testing.T) string {
	t.Helper()
	dir, _ := os.Getwd()
	for i := 0; i < 6; i++ {
		candidate := filepath.Join(dir, "examples")
		if st, err := os.Stat(candidate); err == nil && st.IsDir() {
			return candidate
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}
	t.Fatalf("examples directory not found from %s", dir)
	return ""
}

// loadExamples returns sorted .mty source filenames.
func loadExamples(t *testing.T) []string {
	dir := findExamples(t)
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatalf("read examples dir: %v", err)
	}
	out := []string{}
	for _, e := range entries {
		if !e.IsDir() && strings.HasSuffix(e.Name(), ".mty") {
			out = append(out, filepath.Join(dir, e.Name()))
		}
	}
	return out
}

// TestLexAllExamples checks that every example lexes with zero diagnostics.
// Acceptance criterion per task: all 20 examples lex clean.
func TestLexAllExamples(t *testing.T) {
	for _, path := range loadExamples(t) {
		src, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("read %s: %v", path, err)
		}
		_, diags := Lex(string(src))
		if len(diags) != 0 {
			t.Errorf("%s: lex diagnostics: %v", filepath.Base(path), diags)
		}
	}
}

// TestParseAtLeast10Examples checks the partial-acceptance bar: at least
// 10 of the 20 example files must parse with zero diagnostics.
// Each failure is reported, but the test only fails if fewer than 10 pass.
func TestParseAtLeast10Examples(t *testing.T) {
	examples := loadExamples(t)
	if len(examples) < 20 {
		t.Logf("warning: only %d examples found (expected 20)", len(examples))
	}
	clean := 0
	failed := []string{}
	for _, path := range examples {
		src, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("read %s: %v", path, err)
		}
		_, diags := Parse(string(src))
		base := filepath.Base(path)
		if len(diags) == 0 {
			clean++
			t.Logf("clean: %s", base)
		} else {
			failed = append(failed, base)
			for _, d := range diags[:min(3, len(diags))] {
				t.Logf("  %s: %s", base, d)
			}
		}
	}
	t.Logf("clean-parse: %d / %d", clean, len(examples))
	if clean < 10 {
		t.Fatalf("only %d of %d examples parse cleanly (need ≥10): failed = %v",
			clean, len(examples), failed)
	}
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
