package mty

import (
	"strings"
	"testing"
)

func tokenKinds(toks []Token) []TokenKind {
	out := make([]TokenKind, 0, len(toks))
	for _, t := range toks {
		out = append(out, t.Kind)
	}
	return out
}

func tokenTexts(toks []Token) []string {
	out := make([]string, 0, len(toks))
	for _, t := range toks {
		if t.Kind == TK_EOF {
			continue
		}
		out = append(out, t.Text)
	}
	return out
}

func TestLexEmpty(t *testing.T) {
	toks, diags := Lex("")
	if len(diags) != 0 {
		t.Fatalf("expected no diags, got %v", diags)
	}
	if len(toks) != 1 || toks[0].Kind != TK_EOF {
		t.Fatalf("expected just EOF, got %v", toks)
	}
}

func TestLexKeywords(t *testing.T) {
	src := "fn struct enum impl trait agent protocol sup sandbox"
	toks, diags := Lex(src)
	if len(diags) != 0 {
		t.Fatalf("diags: %v", diags)
	}
	want := []TokenKind{
		TK_KW_FN, TK_KW_STRUCT, TK_KW_ENUM, TK_KW_IMPL, TK_KW_TRAIT,
		TK_KW_AGENT, TK_KW_PROTOCOL, TK_KW_SUP, TK_KW_SANDBOX, TK_EOF,
	}
	got := tokenKinds(toks)
	if !equalKinds(got, want) {
		t.Fatalf("kinds = %v, want %v", got, want)
	}
}

func TestLexIdentVsKeyword(t *testing.T) {
	toks, _ := Lex("foo and panic deinit")
	// `and` `panic` `deinit` are reserved-for-future, so lex as IDENT.
	kinds := tokenKinds(toks)
	if kinds[0] != TK_IDENT || kinds[1] != TK_IDENT || kinds[2] != TK_IDENT || kinds[3] != TK_IDENT {
		t.Fatalf("expected all IDENT, got %v", kinds)
	}
}

func TestLexAllReservedKeywords(t *testing.T) {
	// All 59 reserved keywords must round-trip to keyword kind.
	for kw := range keywordTable {
		toks, diags := Lex(kw)
		if len(diags) != 0 {
			t.Errorf("%q produced diags: %v", kw, diags)
			continue
		}
		if len(toks) != 2 {
			t.Errorf("%q: expected 2 tokens (kw + EOF), got %d", kw, len(toks))
			continue
		}
		if toks[0].Kind == TK_IDENT {
			t.Errorf("%q lexed as IDENT, expected keyword", kw)
		}
	}
}

func TestLexIntegerLiterals(t *testing.T) {
	cases := map[string]bool{
		"0":          true,
		"1_000_000":  true,
		"0xff_ff":    true,
		"0o77":       true,
		"0b1010":     true,
		"42i32":      true,
		"100u64":     true,
		"5usize":     true,
		"7isize":     true,
		"1u128":      true,
	}
	for src := range cases {
		toks, diags := Lex(src)
		if len(diags) != 0 {
			t.Errorf("%q: unexpected diags %v", src, diags)
		}
		if len(toks) != 2 || toks[0].Kind != TK_INT {
			t.Errorf("%q: got %v", src, tokenKinds(toks))
		}
	}
}

func TestLexFloatLiterals(t *testing.T) {
	for _, src := range []string{"3.14", "0.5", "1.0e10", "1.5f32", "2.0f64", "1e10"} {
		toks, diags := Lex(src)
		if len(diags) != 0 {
			t.Errorf("%q: diags: %v", src, diags)
		}
		if toks[0].Kind != TK_FLOAT {
			t.Errorf("%q: expected FLOAT, got %v", src, toks[0].Kind)
		}
	}
}

func TestLexDurations(t *testing.T) {
	for _, src := range []string{"100ms", "5s", "30s", "2h", "1m", "500us", "100ns"} {
		toks, _ := Lex(src)
		if toks[0].Kind != TK_DURATION {
			t.Errorf("%q: expected DURATION, got %v (text=%q)", src, toks[0].Kind, toks[0].Text)
		}
	}
}

func TestLexSizes(t *testing.T) {
	for _, src := range []string{"64MiB", "16MiB", "128MiB", "5k", "10M", "1024B", "2GiB", "32KiB"} {
		toks, _ := Lex(src)
		if toks[0].Kind != TK_SIZE {
			t.Errorf("%q: expected SIZE, got %v (text=%q)", src, toks[0].Kind, toks[0].Text)
		}
	}
}

// Uppercase K should NOT be consumed as a size suffix per §3.4.4.
func TestLexUppercaseKNotSize(t *testing.T) {
	toks, diags := Lex("5K")
	if len(diags) != 0 {
		t.Fatalf("unexpected diags: %v", diags)
	}
	kinds := tokenKinds(toks)
	if len(kinds) != 3 || kinds[0] != TK_INT || kinds[1] != TK_IDENT {
		t.Fatalf("`5K` should lex as INT IDENT, got %v", kinds)
	}
}

func TestLexStrings(t *testing.T) {
	toks, _ := Lex(`"hello, Mighty"`)
	if toks[0].Kind != TK_STRING {
		t.Fatalf("expected STRING, got %v", toks[0].Kind)
	}
	if toks[0].Text != `"hello, Mighty"` {
		t.Fatalf("text mismatch: %q", toks[0].Text)
	}
}

func TestLexStringEscapes(t *testing.T) {
	toks, diags := Lex(`"tab\there\nnewline\u{2603}"`)
	if len(diags) != 0 {
		t.Fatalf("diags: %v", diags)
	}
	if toks[0].Kind != TK_STRING {
		t.Fatalf("expected STRING, got %v", toks[0].Kind)
	}
}

func TestLexRawStrings(t *testing.T) {
	for _, src := range []string{`r"raw"`, `r#"contains "quotes""#`, `r##"with#hashes"##`} {
		toks, diags := Lex(src)
		if len(diags) != 0 {
			t.Errorf("%s: diags %v", src, diags)
		}
		if toks[0].Kind != TK_RAW_STRING {
			t.Errorf("%s: expected RAW_STRING, got %v", src, toks[0].Kind)
		}
	}
}

func TestLexCharLiteral(t *testing.T) {
	toks, _ := Lex(`'a' '\n' '\u{1F600}'`)
	for i := 0; i < 3; i++ {
		if toks[i].Kind != TK_CHAR {
			t.Errorf("toks[%d]: expected CHAR, got %v", i, toks[i].Kind)
		}
	}
}

func TestLexHTMLLiteral(t *testing.T) {
	src := `html"<div>hi {name}</div>"`
	toks, diags := Lex(src)
	if len(diags) != 0 {
		t.Fatalf("diags: %v", diags)
	}
	if toks[0].Kind != TK_HTML_STRING {
		t.Fatalf("expected HTML_STRING, got %v", toks[0].Kind)
	}
}

func TestLexComments(t *testing.T) {
	src := `// line
/* block */
/// doc
//// banner`
	l := NewLexer(src)
	l.KeepTrivia(true)
	toks, _ := l.Lex()
	// Filter out whitespace.
	var kinds []TokenKind
	for _, t := range toks {
		if t.Kind != TK_WHITESPACE && t.Kind != TK_EOF {
			kinds = append(kinds, t.Kind)
		}
	}
	want := []TokenKind{TK_LINE_COMMENT, TK_BLOCK_COMMENT, TK_DOC_COMMENT, TK_LINE_COMMENT}
	if !equalKinds(kinds, want) {
		t.Fatalf("kinds = %v, want %v", kinds, want)
	}
}

func TestLexNestedBlockComment(t *testing.T) {
	toks, diags := Lex(`/* outer /* inner */ still outer */`)
	if len(diags) != 0 {
		t.Fatalf("unexpected diags: %v", diags)
	}
	if toks[0].Kind != TK_EOF {
		t.Fatalf("expected just EOF (trivia dropped), got %v", toks[0].Kind)
	}
}

func TestLexUnterminatedBlockComment(t *testing.T) {
	_, diags := Lex(`/* unterminated`)
	if len(diags) == 0 || diags[0].Code != "MT0004" {
		t.Fatalf("expected MT0004, got %v", diags)
	}
}

func TestLexPunctuation(t *testing.T) {
	src := "( ) { } [ ] , ; : :: . .. ..= -> => @ # $ ? ! & && | || ^ ~ = == != < <= > >= + += - -= * *= / /= % %= << >>"
	toks, diags := Lex(src)
	if len(diags) != 0 {
		t.Fatalf("diags: %v", diags)
	}
	want := []TokenKind{
		TK_LPAREN, TK_RPAREN, TK_LBRACE, TK_RBRACE, TK_LBRACK, TK_RBRACK,
		TK_COMMA, TK_SEMI, TK_COLON, TK_DCOLON, TK_DOT, TK_DOTDOT, TK_DOTDOTEQ,
		TK_ARROW, TK_FATARROW, TK_AT, TK_HASH, TK_DOLLAR, TK_QUESTION, TK_BANG,
		TK_AMP, TK_AMPAMP, TK_PIPE, TK_PIPEPIPE, TK_CARET, TK_TILDE,
		TK_EQ, TK_EQEQ, TK_NE, TK_LT, TK_LE, TK_GT, TK_GE,
		TK_PLUS, TK_PLUSEQ, TK_MINUS, TK_MINUSEQ, TK_STAR, TK_STAREQ,
		TK_SLASH, TK_SLASHEQ, TK_PERCENT, TK_PERCENTEQ, TK_SHL, TK_SHR, TK_EOF,
	}
	got := tokenKinds(toks)
	if !equalKinds(got, want) {
		t.Fatalf("\n got=%v\nwant=%v", got, want)
	}
}

func TestLexCRLFNormalisation(t *testing.T) {
	src := "fn\r\nfoo\r\n"
	toks, _ := Lex(src)
	if toks[0].Kind != TK_KW_FN || toks[1].Kind != TK_IDENT {
		t.Fatalf("got %v", tokenKinds(toks))
	}
}

func TestLexBOMRejected(t *testing.T) {
	src := "\xEF\xBB\xBFfn"
	_, diags := Lex(src)
	if len(diags) == 0 || diags[0].Code != "MT0002" {
		t.Fatalf("expected MT0002 BOM, got %v", diags)
	}
}

func TestLexMT0006TrailingUnderscore(t *testing.T) {
	_, diags := Lex("1_")
	found := false
	for _, d := range diags {
		if d.Code == "MT0006" {
			found = true
		}
	}
	if !found {
		t.Fatalf("expected MT0006 for `1_`, got diags=%v", diags)
	}
}

func TestLexMT0006DoubleUnderscore(t *testing.T) {
	_, diags := Lex("1__2")
	found := false
	for _, d := range diags {
		if d.Code == "MT0006" {
			found = true
		}
	}
	if !found {
		t.Fatalf("expected MT0006 for `1__2`, got %v", diags)
	}
}

func TestLexUnderscoreIdent(t *testing.T) {
	toks, _ := Lex("_foo")
	if toks[0].Kind != TK_IDENT || toks[0].Text != "_foo" {
		t.Fatalf("got %v", toks[0])
	}
}

func TestLexSpansAreByteAccurate(t *testing.T) {
	src := "let x = 42"
	toks, _ := Lex(src)
	if toks[0].Start != 0 || toks[0].End != 3 {
		t.Fatalf("`let` span = %d..%d, want 0..3", toks[0].Start, toks[0].End)
	}
	if !strings.HasPrefix(src[toks[1].Start:toks[1].End], "x") {
		t.Fatalf("second token mismatch: %q", src[toks[1].Start:toks[1].End])
	}
}

func equalKinds(a, b []TokenKind) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
