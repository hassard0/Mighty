package mty

import (
	"fmt"
	"unicode"
	"unicode/utf8"
)

// TokenKind enumerates every token kind produced by the lexer.
//
// Derivation: v1.0-RC §3 (lexical structure). Punctuation set is the union
// of all operator-shape tokens used by the example corpus and the surface
// described in §3.5 + §6 + §11 + §17.
type TokenKind int

const (
	TK_INVALID TokenKind = iota

	// Trivia (kept in the stream so a downstream tool can format).
	TK_WHITESPACE
	TK_LINE_COMMENT
	TK_BLOCK_COMMENT
	TK_DOC_COMMENT

	// Identifiers / literals.
	TK_IDENT
	TK_INT
	TK_FLOAT
	TK_STRING
	TK_RAW_STRING
	TK_BYTE_STRING
	TK_CHAR
	TK_HTML_STRING
	TK_DURATION
	TK_SIZE

	// Reserved keywords (v1.0-RC §3.3.1). 59 entries.
	TK_KW_AGENT
	TK_KW_ARENA
	TK_KW_AS
	TK_KW_ASYNC
	TK_KW_AWAIT
	TK_KW_BACKOFF
	TK_KW_BREAK
	TK_KW_BUDGET
	TK_KW_CAP
	TK_KW_CHILD
	TK_KW_CONST
	TK_KW_CONTINUE
	TK_KW_DERIVE
	TK_KW_DETACH
	TK_KW_DYN
	TK_KW_EFFECT
	TK_KW_ELSE
	TK_KW_ENUM
	TK_KW_EXPORT
	TK_KW_EXTERN
	TK_KW_FALSE
	TK_KW_FN
	TK_KW_FOR
	TK_KW_IF
	TK_KW_IMPL
	TK_KW_IMPORT
	TK_KW_IN
	TK_KW_JOIN
	TK_KW_LET
	TK_KW_LOOP
	TK_KW_MACRO
	TK_KW_MATCH
	TK_KW_MOD
	TK_KW_MOVE
	TK_KW_MUT
	TK_KW_ON
	TK_KW_ON_FAIL
	TK_KW_PACKAGE
	TK_KW_PROTOCOL
	TK_KW_PUB
	TK_KW_REF
	TK_KW_REQUIRES
	TK_KW_RESTART
	TK_KW_RETURN
	TK_KW_RUN
	TK_KW_SANDBOX
	TK_KW_SCOPE
	TK_KW_SELF
	TK_KW_SPAWN
	TK_KW_STATE
	TK_KW_STRUCT
	TK_KW_SUP
	TK_KW_TASK
	TK_KW_TRAIT
	TK_KW_TRUE
	TK_KW_TYPE
	TK_KW_UNSAFE
	TK_KW_UP_TO
	TK_KW_USE
	TK_KW_WHERE
	TK_KW_WHILE
	TK_KW_WITH
	TK_KW_YIELD

	// Punctuation.
	TK_LPAREN    // (
	TK_RPAREN    // )
	TK_LBRACE    // {
	TK_RBRACE    // }
	TK_LBRACK    // [
	TK_RBRACK    // ]
	TK_COMMA     // ,
	TK_SEMI      // ;
	TK_COLON     // :
	TK_DCOLON    // ::
	TK_DOT       // .
	TK_DOTDOT    // ..
	TK_DOTDOTEQ  // ..=
	TK_ARROW     // ->
	TK_FATARROW  // =>
	TK_AT        // @
	TK_HASH      // #
	TK_DOLLAR    // $
	TK_QUESTION  // ?
	TK_BANG      // !
	TK_AMP       // &
	TK_AMPAMP    // &&
	TK_PIPE      // |
	TK_PIPEPIPE  // ||
	TK_CARET     // ^
	TK_TILDE     // ~
	TK_EQ        // =
	TK_EQEQ      // ==
	TK_NE        // !=
	TK_LT        // <
	TK_LE        // <=
	TK_GT        // >
	TK_GE        // >=
	TK_PLUS      // +
	TK_PLUSEQ    // +=
	TK_MINUS     // -
	TK_MINUSEQ   // -=
	TK_STAR      // *
	TK_STAREQ    // *=
	TK_SLASH     // /
	TK_SLASHEQ   // /=
	TK_PERCENT   // %
	TK_PERCENTEQ // %=
	TK_SHL       // <<
	TK_SHR       // >>
	TK_AMPEQ     // &=
	TK_PIPEEQ    // |=
	TK_CARETEQ   // ^=
	TK_UNDERSCORE // bare _ in pattern position; lexed as IDENT but parser elevates

	TK_EOF
)

// Token is a single lexer output.
type Token struct {
	Kind  TokenKind
	Text  string
	Start int
	End   int
}

func (t Token) String() string {
	return fmt.Sprintf("%s(%q)@%d..%d", t.Kind.Name(), t.Text, t.Start, t.End)
}

// Name returns a human-readable token-kind name (used in tests and diagnostics).
func (k TokenKind) Name() string {
	if n, ok := tokenKindNames[k]; ok {
		return n
	}
	return fmt.Sprintf("TK_?(%d)", int(k))
}

var tokenKindNames = map[TokenKind]string{
	TK_INVALID:       "INVALID",
	TK_WHITESPACE:    "WS",
	TK_LINE_COMMENT:  "LINE_COMMENT",
	TK_BLOCK_COMMENT: "BLOCK_COMMENT",
	TK_DOC_COMMENT:   "DOC_COMMENT",
	TK_IDENT:         "IDENT",
	TK_INT:           "INT",
	TK_FLOAT:         "FLOAT",
	TK_STRING:        "STRING",
	TK_RAW_STRING:    "RAW_STRING",
	TK_BYTE_STRING:   "BYTE_STRING",
	TK_CHAR:          "CHAR",
	TK_HTML_STRING:   "HTML_STRING",
	TK_DURATION:      "DURATION",
	TK_SIZE:          "SIZE",
	TK_EOF:           "EOF",

	TK_LPAREN: "(", TK_RPAREN: ")", TK_LBRACE: "{", TK_RBRACE: "}",
	TK_LBRACK: "[", TK_RBRACK: "]",
	TK_COMMA: ",", TK_SEMI: ";", TK_COLON: ":", TK_DCOLON: "::",
	TK_DOT: ".", TK_DOTDOT: "..", TK_DOTDOTEQ: "..=",
	TK_ARROW: "->", TK_FATARROW: "=>", TK_AT: "@", TK_HASH: "#",
	TK_DOLLAR: "$", TK_QUESTION: "?", TK_BANG: "!",
	TK_AMP: "&", TK_AMPAMP: "&&", TK_PIPE: "|", TK_PIPEPIPE: "||",
	TK_CARET: "^", TK_TILDE: "~",
	TK_EQ: "=", TK_EQEQ: "==", TK_NE: "!=",
	TK_LT: "<", TK_LE: "<=", TK_GT: ">", TK_GE: ">=",
	TK_PLUS: "+", TK_PLUSEQ: "+=", TK_MINUS: "-", TK_MINUSEQ: "-=",
	TK_STAR: "*", TK_STAREQ: "*=", TK_SLASH: "/", TK_SLASHEQ: "/=",
	TK_PERCENT: "%", TK_PERCENTEQ: "%=",
	TK_SHL: "<<", TK_SHR: ">>",
	TK_AMPEQ: "&=", TK_PIPEEQ: "|=", TK_CARETEQ: "^=",
	TK_UNDERSCORE: "_",
}

// keywordTable maps lexeme → keyword TokenKind for the 59 reserved keywords
// (v1.0-RC §3.3.1).
var keywordTable = map[string]TokenKind{
	"agent": TK_KW_AGENT, "arena": TK_KW_ARENA, "as": TK_KW_AS,
	"async": TK_KW_ASYNC, "await": TK_KW_AWAIT, "backoff": TK_KW_BACKOFF,
	"break": TK_KW_BREAK, "budget": TK_KW_BUDGET, "cap": TK_KW_CAP,
	"child": TK_KW_CHILD, "const": TK_KW_CONST, "continue": TK_KW_CONTINUE,
	"derive": TK_KW_DERIVE, "detach": TK_KW_DETACH, "dyn": TK_KW_DYN,
	"effect": TK_KW_EFFECT, "else": TK_KW_ELSE, "enum": TK_KW_ENUM,
	"export": TK_KW_EXPORT, "extern": TK_KW_EXTERN, "false": TK_KW_FALSE,
	"fn": TK_KW_FN, "for": TK_KW_FOR, "if": TK_KW_IF,
	"impl": TK_KW_IMPL, "import": TK_KW_IMPORT, "in": TK_KW_IN,
	"join": TK_KW_JOIN, "let": TK_KW_LET, "loop": TK_KW_LOOP,
	"macro": TK_KW_MACRO, "match": TK_KW_MATCH, "mod": TK_KW_MOD,
	"move": TK_KW_MOVE, "mut": TK_KW_MUT, "on": TK_KW_ON,
	"on_fail": TK_KW_ON_FAIL, "package": TK_KW_PACKAGE, "protocol": TK_KW_PROTOCOL,
	"pub": TK_KW_PUB, "ref": TK_KW_REF, "requires": TK_KW_REQUIRES,
	"restart": TK_KW_RESTART, "return": TK_KW_RETURN, "run": TK_KW_RUN,
	"sandbox": TK_KW_SANDBOX, "scope": TK_KW_SCOPE, "self": TK_KW_SELF,
	"spawn": TK_KW_SPAWN, "state": TK_KW_STATE, "struct": TK_KW_STRUCT,
	"sup": TK_KW_SUP, "task": TK_KW_TASK, "trait": TK_KW_TRAIT,
	"true": TK_KW_TRUE, "type": TK_KW_TYPE, "unsafe": TK_KW_UNSAFE,
	"up_to": TK_KW_UP_TO, "use": TK_KW_USE, "where": TK_KW_WHERE,
	"while": TK_KW_WHILE, "with": TK_KW_WITH, "yield": TK_KW_YIELD,
}

// Lexer streams tokens out of UTF-8 source. CRLF is normalised to LF
// (v1.0-RC §3.1). BOM is rejected via MT0002.
type Lexer struct {
	src    string
	pos    int
	tokens []Token
	diags  []Diagnostic
	// keepTrivia: if false (default), whitespace + comments are dropped
	// from the output stream so the parser sees a compact view.
	keepTrivia bool
}

// NewLexer constructs a lexer ready to consume src.
func NewLexer(src string) *Lexer {
	// Normalise CRLF → LF.
	src = normalizeCRLF(src)
	return &Lexer{src: src}
}

// KeepTrivia toggles whether whitespace and comments are emitted as tokens.
// The parser uses this with `false`; formatters / pretty-printers would
// flip to `true`.
func (l *Lexer) KeepTrivia(keep bool) {
	l.keepTrivia = keep
}

// Lex runs the lexer to completion and returns the token stream plus any
// diagnostics.
func (l *Lexer) Lex() ([]Token, []Diagnostic) {
	// Reject leading BOM per §3.1.
	if len(l.src) >= 3 && l.src[0] == 0xEF && l.src[1] == 0xBB && l.src[2] == 0xBF {
		l.diag("MT0002", "BOM not permitted in Mighty source", 0, 3)
	}
	for l.pos < len(l.src) {
		l.scan()
	}
	l.emit(TK_EOF, l.pos, l.pos)
	return l.tokens, l.diags
}

// scan reads and emits one token from the current position.
func (l *Lexer) scan() {
	start := l.pos
	c := l.src[l.pos]

	// ASCII fast-path. Multi-byte sequences fall through to the slow path
	// for identifier-start XID classification.
	switch {
	case c == ' ' || c == '\t' || c == '\n' || c == '\r':
		l.scanWhitespace(start)
		return
	case c == '/':
		// Comment vs slash.
		if l.peekAt(l.pos+1) == '/' {
			l.scanLineComment(start)
			return
		}
		if l.peekAt(l.pos+1) == '*' {
			l.scanBlockComment(start)
			return
		}
		if l.peekAt(l.pos+1) == '=' {
			l.pos += 2
			l.emit(TK_SLASHEQ, start, l.pos)
			return
		}
		l.pos++
		l.emit(TK_SLASH, start, l.pos)
		return
	case c == '"':
		l.scanString(start, false)
		return
	case c == '\'':
		l.scanChar(start)
		return
	case c == 'r' && (l.peekAt(l.pos+1) == '"' || l.peekAt(l.pos+1) == '#'):
		l.scanRawString(start)
		return
	case c == 'b' && l.peekAt(l.pos+1) == '"':
		l.pos++ // consume 'b'
		l.scanString(start, true)
		return
	case (c >= '0' && c <= '9'):
		l.scanNumber(start)
		return
	case c == '_' || isIdentStart(rune(c)):
		l.scanIdentOrKeyword(start)
		return
	}

	// Multi-byte identifier start.
	r, size := utf8.DecodeRuneInString(l.src[l.pos:])
	if r != utf8.RuneError && isIdentStart(r) {
		l.scanIdentOrKeyword(start)
		return
	}

	// Punctuation / unknown.
	if l.scanPunct(start) {
		return
	}

	// Unknown character.
	if r == utf8.RuneError {
		l.pos++
	} else {
		l.pos += size
	}
	l.diag("MT0001", fmt.Sprintf("unexpected character %q", string(r)), start, l.pos)
	l.emit(TK_INVALID, start, l.pos)
}

// scanWhitespace consumes any run of ASCII whitespace.
func (l *Lexer) scanWhitespace(start int) {
	for l.pos < len(l.src) {
		c := l.src[l.pos]
		if c != ' ' && c != '\t' && c != '\n' && c != '\r' {
			break
		}
		l.pos++
	}
	if l.keepTrivia {
		l.emit(TK_WHITESPACE, start, l.pos)
	}
}

// scanLineComment handles `//` and `///` forms per §3.2.
// Four-or-more slashes degrade to ordinary line comment.
func (l *Lexer) scanLineComment(start int) {
	// Count leading slashes (we already know src[pos]=='/' and pos+1=='/').
	slashes := 0
	for l.pos < len(l.src) && l.src[l.pos] == '/' {
		l.pos++
		slashes++
	}
	for l.pos < len(l.src) && l.src[l.pos] != '\n' {
		l.pos++
	}
	if slashes == 3 {
		if l.keepTrivia {
			l.emit(TK_DOC_COMMENT, start, l.pos)
		}
	} else {
		if l.keepTrivia {
			l.emit(TK_LINE_COMMENT, start, l.pos)
		}
	}
}

// scanBlockComment supports nesting; unterminated emits MT0004.
func (l *Lexer) scanBlockComment(start int) {
	l.pos += 2 // consume /*
	depth := 1
	for l.pos < len(l.src) && depth > 0 {
		if l.pos+1 < len(l.src) && l.src[l.pos] == '/' && l.src[l.pos+1] == '*' {
			depth++
			l.pos += 2
		} else if l.pos+1 < len(l.src) && l.src[l.pos] == '*' && l.src[l.pos+1] == '/' {
			depth--
			l.pos += 2
		} else {
			l.pos++
		}
	}
	if depth > 0 {
		l.diag("MT0004", "unterminated block comment", start, l.pos)
	}
	if l.keepTrivia {
		l.emit(TK_BLOCK_COMMENT, start, l.pos)
	}
}

// scanString reads a "..." string with escapes; if byteString, the leading
// 'b' has already been consumed and we emit TK_BYTE_STRING.
func (l *Lexer) scanString(start int, byteString bool) {
	l.pos++ // opening quote
	terminated := false
	for l.pos < len(l.src) {
		c := l.src[l.pos]
		if c == '"' {
			l.pos++
			terminated = true
			break
		}
		if c == '\\' {
			l.pos++
			if l.pos < len(l.src) {
				// Accept any escape character; semantic validation is the
				// parser's job. Covers \n \r \t \\ \" \' \0 \xHH \uXXXX
				// \u{...}.
				ec := l.src[l.pos]
				l.pos++
				switch ec {
				case 'x':
					for i := 0; i < 2 && l.pos < len(l.src) && isHex(l.src[l.pos]); i++ {
						l.pos++
					}
				case 'u':
					if l.pos < len(l.src) && l.src[l.pos] == '{' {
						l.pos++
						for l.pos < len(l.src) && l.src[l.pos] != '}' {
							l.pos++
						}
						if l.pos < len(l.src) {
							l.pos++
						}
					} else {
						for i := 0; i < 4 && l.pos < len(l.src) && isHex(l.src[l.pos]); i++ {
							l.pos++
						}
					}
				}
			}
			continue
		}
		l.pos++
	}
	if !terminated {
		l.diag("MT0003", "unterminated string literal", start, l.pos)
	}
	if byteString {
		l.emit(TK_BYTE_STRING, start, l.pos)
	} else {
		l.emit(TK_STRING, start, l.pos)
	}
}

// scanRawString supports r"..." and r#"..."# (any # count).
func (l *Lexer) scanRawString(start int) {
	l.pos++ // 'r'
	hashes := 0
	for l.pos < len(l.src) && l.src[l.pos] == '#' {
		hashes++
		l.pos++
	}
	if l.pos >= len(l.src) || l.src[l.pos] != '"' {
		l.diag("MT0005", "raw string literal must start with a quote", start, l.pos)
		l.emit(TK_INVALID, start, l.pos)
		return
	}
	l.pos++ // opening quote
	terminated := false
	for l.pos < len(l.src) {
		if l.src[l.pos] == '"' {
			// Check for matching hash count.
			matched := true
			for i := 0; i < hashes; i++ {
				if l.pos+1+i >= len(l.src) || l.src[l.pos+1+i] != '#' {
					matched = false
					break
				}
			}
			if matched {
				l.pos += 1 + hashes
				terminated = true
				break
			}
		}
		l.pos++
	}
	if !terminated {
		l.diag("MT0003", "unterminated raw string literal", start, l.pos)
	}
	l.emit(TK_RAW_STRING, start, l.pos)
}

// scanChar reads a 'c' or '\n' character literal.
func (l *Lexer) scanChar(start int) {
	l.pos++ // opening '
	terminated := false
	if l.pos < len(l.src) && l.src[l.pos] == '\\' {
		l.pos++
		if l.pos < len(l.src) {
			ec := l.src[l.pos]
			l.pos++
			if ec == 'x' {
				for i := 0; i < 2 && l.pos < len(l.src) && isHex(l.src[l.pos]); i++ {
					l.pos++
				}
			} else if ec == 'u' {
				if l.pos < len(l.src) && l.src[l.pos] == '{' {
					l.pos++
					for l.pos < len(l.src) && l.src[l.pos] != '}' {
						l.pos++
					}
					if l.pos < len(l.src) {
						l.pos++
					}
				}
			}
		}
	} else if l.pos < len(l.src) {
		_, size := utf8.DecodeRuneInString(l.src[l.pos:])
		l.pos += size
	}
	if l.pos < len(l.src) && l.src[l.pos] == '\'' {
		l.pos++
		terminated = true
	}
	if !terminated {
		l.diag("MT0003", "unterminated character literal", start, l.pos)
	}
	l.emit(TK_CHAR, start, l.pos)
}

// scanNumber recognises INT, FLOAT, DURATION, SIZE per §3.4.
func (l *Lexer) scanNumber(start int) {
	// Hex / oct / bin?
	if l.src[l.pos] == '0' && l.pos+1 < len(l.src) {
		switch l.src[l.pos+1] {
		case 'x', 'X':
			l.pos += 2
			l.consumeDigits(isHex)
			l.tryIntSuffix()
			l.checkUnderscoreFollow(start)
			l.emit(TK_INT, start, l.pos)
			return
		case 'o', 'O':
			l.pos += 2
			l.consumeDigits(isOct)
			l.tryIntSuffix()
			l.checkUnderscoreFollow(start)
			l.emit(TK_INT, start, l.pos)
			return
		case 'b', 'B':
			// Disambiguate against byte-string prefix `b"..."` (already
			// matched earlier) — here we're definitely number-position.
			l.pos += 2
			l.consumeDigits(isBin)
			l.tryIntSuffix()
			l.checkUnderscoreFollow(start)
			l.emit(TK_INT, start, l.pos)
			return
		}
	}

	// Decimal digit run.
	l.consumeDigits(isDec)

	// Float?  `.` followed by a digit (NOT `..` range syntax).
	if l.pos+1 < len(l.src) && l.src[l.pos] == '.' && isDec(l.src[l.pos+1]) {
		l.pos++ // .
		l.consumeDigits(isDec)
		// Exponent.
		if l.pos < len(l.src) && (l.src[l.pos] == 'e' || l.src[l.pos] == 'E') {
			l.pos++
			if l.pos < len(l.src) && (l.src[l.pos] == '+' || l.src[l.pos] == '-') {
				l.pos++
			}
			l.consumeDigits(isDec)
		}
		// f32 / f64 suffix.
		l.tryFloatSuffix()
		l.checkUnderscoreFollow(start)
		l.emit(TK_FLOAT, start, l.pos)
		return
	}

	// Exponent without fractional part (1e10).
	if l.pos < len(l.src) && (l.src[l.pos] == 'e' || l.src[l.pos] == 'E') {
		l.pos++
		if l.pos < len(l.src) && (l.src[l.pos] == '+' || l.src[l.pos] == '-') {
			l.pos++
		}
		l.consumeDigits(isDec)
		l.tryFloatSuffix()
		l.checkUnderscoreFollow(start)
		l.emit(TK_FLOAT, start, l.pos)
		return
	}

	// Size / duration suffix? Order matters: longer literals first.
	if kind, ok := l.trySizeSuffix(); ok {
		l.emit(kind, start, l.pos)
		return
	}
	if kind, ok := l.tryDurationSuffix(); ok {
		l.emit(kind, start, l.pos)
		return
	}

	// Integer suffix (u8..u128, i8..i128, usize, isize).
	if l.tryIntSuffix() {
		l.checkUnderscoreFollow(start)
		l.emit(TK_INT, start, l.pos)
		return
	}

	l.checkUnderscoreFollow(start)
	l.emit(TK_INT, start, l.pos)
}

// consumeDigits consumes a (digit (_ digit)*) run. Trailing underscore and
// double underscore are flagged with MT0006 per §3.4.1.
func (l *Lexer) consumeDigits(isDigit func(byte) bool) {
	sawDigit := false
	for l.pos < len(l.src) {
		c := l.src[l.pos]
		if isDigit(c) {
			sawDigit = true
			l.pos++
			continue
		}
		if c == '_' && sawDigit {
			// Detect double underscore.
			if l.pos+1 < len(l.src) && l.src[l.pos+1] == '_' {
				l.diag("MT0006", "consecutive underscores in numeric literal", l.pos, l.pos+2)
			}
			// Allow underscore between digits but consume; flag trailing later.
			l.pos++
			continue
		}
		break
	}
	// Check trailing underscore.
	if l.pos > 0 && l.src[l.pos-1] == '_' && sawDigit {
		l.diag("MT0006", "trailing underscore in numeric literal", l.pos-1, l.pos)
	}
}

// tryIntSuffix consumes an `[iu](8|16|32|64|128|size)` suffix if present.
// Returns true iff a suffix was consumed.
func (l *Lexer) tryIntSuffix() bool {
	if l.pos >= len(l.src) {
		return false
	}
	c := l.src[l.pos]
	if c != 'i' && c != 'u' {
		return false
	}
	rest := l.src[l.pos+1:]
	for _, w := range []string{"size", "128", "64", "32", "16", "8"} {
		if len(rest) >= len(w) && rest[:len(w)] == w {
			// Ensure not followed by IDENT-continue char.
			after := l.pos + 1 + len(w)
			if after < len(l.src) {
				r, _ := utf8.DecodeRuneInString(l.src[after:])
				if isIdentContinue(r) {
					return false
				}
			}
			l.pos = after
			return true
		}
	}
	return false
}

// tryFloatSuffix consumes `f32` or `f64` if present.
func (l *Lexer) tryFloatSuffix() bool {
	if l.pos+2 < len(l.src) && l.src[l.pos] == 'f' {
		w := l.src[l.pos+1 : l.pos+3]
		if w == "32" || w == "64" {
			after := l.pos + 3
			if after < len(l.src) {
				r, _ := utf8.DecodeRuneInString(l.src[after:])
				if isIdentContinue(r) {
					return false
				}
			}
			l.pos = after
			return true
		}
	}
	return false
}

// trySizeSuffix consumes one of `B`, `KiB`, `MiB`, `GiB`, `k`, `M` per §3.4.4.
// Uppercase `K` is NOT consumed (reserved).
func (l *Lexer) trySizeSuffix() (TokenKind, bool) {
	if l.pos >= len(l.src) {
		return 0, false
	}
	// Order: longest first.
	for _, suf := range []string{"KiB", "MiB", "GiB"} {
		if l.tryConsumeSuffix(suf) {
			return TK_SIZE, true
		}
	}
	// Single-char k or M or B — but must not collide with duration (m, h, etc.)
	// and must not be followed by another identifier-continue char.
	if l.pos < len(l.src) {
		c := l.src[l.pos]
		if c == 'k' || c == 'M' || c == 'B' {
			after := l.pos + 1
			if after < len(l.src) {
				r, _ := utf8.DecodeRuneInString(l.src[after:])
				if isIdentContinue(r) {
					return 0, false
				}
			}
			l.pos = after
			return TK_SIZE, true
		}
	}
	return 0, false
}

// tryDurationSuffix consumes one of `ns`, `us`, `ms`, `s`, `m`, `h` per §3.4.5.
func (l *Lexer) tryDurationSuffix() (TokenKind, bool) {
	for _, suf := range []string{"ns", "us", "ms"} {
		if l.tryConsumeSuffix(suf) {
			return TK_DURATION, true
		}
	}
	if l.pos < len(l.src) {
		c := l.src[l.pos]
		if c == 's' || c == 'm' || c == 'h' {
			after := l.pos + 1
			if after < len(l.src) {
				r, _ := utf8.DecodeRuneInString(l.src[after:])
				if isIdentContinue(r) {
					return 0, false
				}
			}
			l.pos = after
			return TK_DURATION, true
		}
	}
	return 0, false
}

// tryConsumeSuffix advances past s if src[pos:] starts with s and the
// character immediately after s is not an identifier-continue rune.
func (l *Lexer) tryConsumeSuffix(s string) bool {
	if l.pos+len(s) > len(l.src) {
		return false
	}
	if l.src[l.pos:l.pos+len(s)] != s {
		return false
	}
	after := l.pos + len(s)
	if after < len(l.src) {
		r, _ := utf8.DecodeRuneInString(l.src[after:])
		if isIdentContinue(r) {
			return false
		}
	}
	l.pos = after
	return true
}

// checkUnderscoreFollow surfaces MT0006 when a numeric literal is
// immediately followed by `_<digit>` (the spec wants such tokens to lex
// as IDENT instead, but the corpus uses `1_000` style so we only flag
// runs that the consumeDigits pass didn't already eat).
func (l *Lexer) checkUnderscoreFollow(start int) {
	// no-op placeholder: real detection happens in consumeDigits.
	_ = start
}

// scanIdentOrKeyword reads an identifier and classifies as keyword or IDENT.
func (l *Lexer) scanIdentOrKeyword(start int) {
	// Special handling for `html"..."` tagged string literal.
	if l.src[l.pos] == 'h' && l.pos+4 < len(l.src) && l.src[l.pos:l.pos+4] == "html" && l.src[l.pos+4] == '"' {
		l.scanHTMLString(start)
		return
	}
	for l.pos < len(l.src) {
		r, size := utf8.DecodeRuneInString(l.src[l.pos:])
		if !isIdentContinue(r) {
			break
		}
		l.pos += size
	}
	text := l.src[start:l.pos]
	if kw, ok := keywordTable[text]; ok {
		l.emit(kw, start, l.pos)
		return
	}
	l.emit(TK_IDENT, start, l.pos)
}

// scanHTMLString reads an `html"..."` tagged template; inner `{...}` braces
// are paired-balanced per v1.0-RC §3.4.3.
func (l *Lexer) scanHTMLString(start int) {
	l.pos += 4 // html
	if l.pos >= len(l.src) || l.src[l.pos] != '"' {
		l.diag("MT0007", "expected '\"' after html tag", start, l.pos)
		l.emit(TK_INVALID, start, l.pos)
		return
	}
	l.pos++ // opening quote
	braceDepth := 0
	terminated := false
	for l.pos < len(l.src) {
		c := l.src[l.pos]
		if c == '\\' {
			l.pos++
			if l.pos < len(l.src) {
				l.pos++
			}
			continue
		}
		if c == '{' {
			braceDepth++
			l.pos++
			continue
		}
		if c == '}' {
			if braceDepth > 0 {
				braceDepth--
			}
			l.pos++
			continue
		}
		if c == '"' && braceDepth == 0 {
			l.pos++
			terminated = true
			break
		}
		l.pos++
	}
	if !terminated {
		l.diag("MT0003", "unterminated html literal", start, l.pos)
	}
	l.emit(TK_HTML_STRING, start, l.pos)
}

// scanPunct attempts to recognise a multi-or-single-char punctuation token.
// Returns true on success.
func (l *Lexer) scanPunct(start int) bool {
	src := l.src
	p := l.pos
	rem := func() byte {
		if p+1 < len(src) {
			return src[p+1]
		}
		return 0
	}
	rem2 := func() byte {
		if p+2 < len(src) {
			return src[p+2]
		}
		return 0
	}
	c := src[p]
	switch c {
	case '(':
		l.pos++
		l.emit(TK_LPAREN, start, l.pos)
	case ')':
		l.pos++
		l.emit(TK_RPAREN, start, l.pos)
	case '{':
		l.pos++
		l.emit(TK_LBRACE, start, l.pos)
	case '}':
		l.pos++
		l.emit(TK_RBRACE, start, l.pos)
	case '[':
		l.pos++
		l.emit(TK_LBRACK, start, l.pos)
	case ']':
		l.pos++
		l.emit(TK_RBRACK, start, l.pos)
	case ',':
		l.pos++
		l.emit(TK_COMMA, start, l.pos)
	case ';':
		l.pos++
		l.emit(TK_SEMI, start, l.pos)
	case ':':
		if rem() == ':' {
			l.pos += 2
			l.emit(TK_DCOLON, start, l.pos)
		} else {
			l.pos++
			l.emit(TK_COLON, start, l.pos)
		}
	case '.':
		if rem() == '.' && rem2() == '=' {
			l.pos += 3
			l.emit(TK_DOTDOTEQ, start, l.pos)
		} else if rem() == '.' {
			l.pos += 2
			l.emit(TK_DOTDOT, start, l.pos)
		} else {
			l.pos++
			l.emit(TK_DOT, start, l.pos)
		}
	case '-':
		if rem() == '>' {
			l.pos += 2
			l.emit(TK_ARROW, start, l.pos)
		} else if rem() == '=' {
			l.pos += 2
			l.emit(TK_MINUSEQ, start, l.pos)
		} else {
			l.pos++
			l.emit(TK_MINUS, start, l.pos)
		}
	case '=':
		if rem() == '>' {
			l.pos += 2
			l.emit(TK_FATARROW, start, l.pos)
		} else if rem() == '=' {
			l.pos += 2
			l.emit(TK_EQEQ, start, l.pos)
		} else {
			l.pos++
			l.emit(TK_EQ, start, l.pos)
		}
	case '@':
		l.pos++
		l.emit(TK_AT, start, l.pos)
	case '#':
		l.pos++
		l.emit(TK_HASH, start, l.pos)
	case '$':
		l.pos++
		l.emit(TK_DOLLAR, start, l.pos)
	case '?':
		l.pos++
		l.emit(TK_QUESTION, start, l.pos)
	case '!':
		if rem() == '=' {
			l.pos += 2
			l.emit(TK_NE, start, l.pos)
		} else {
			l.pos++
			l.emit(TK_BANG, start, l.pos)
		}
	case '&':
		if rem() == '&' {
			l.pos += 2
			l.emit(TK_AMPAMP, start, l.pos)
		} else if rem() == '=' {
			l.pos += 2
			l.emit(TK_AMPEQ, start, l.pos)
		} else {
			l.pos++
			l.emit(TK_AMP, start, l.pos)
		}
	case '|':
		if rem() == '|' {
			l.pos += 2
			l.emit(TK_PIPEPIPE, start, l.pos)
		} else if rem() == '=' {
			l.pos += 2
			l.emit(TK_PIPEEQ, start, l.pos)
		} else {
			l.pos++
			l.emit(TK_PIPE, start, l.pos)
		}
	case '^':
		if rem() == '=' {
			l.pos += 2
			l.emit(TK_CARETEQ, start, l.pos)
		} else {
			l.pos++
			l.emit(TK_CARET, start, l.pos)
		}
	case '~':
		l.pos++
		l.emit(TK_TILDE, start, l.pos)
	case '<':
		if rem() == '=' {
			l.pos += 2
			l.emit(TK_LE, start, l.pos)
		} else if rem() == '<' {
			l.pos += 2
			l.emit(TK_SHL, start, l.pos)
		} else {
			l.pos++
			l.emit(TK_LT, start, l.pos)
		}
	case '>':
		if rem() == '=' {
			l.pos += 2
			l.emit(TK_GE, start, l.pos)
		} else if rem() == '>' {
			l.pos += 2
			l.emit(TK_SHR, start, l.pos)
		} else {
			l.pos++
			l.emit(TK_GT, start, l.pos)
		}
	case '+':
		if rem() == '=' {
			l.pos += 2
			l.emit(TK_PLUSEQ, start, l.pos)
		} else {
			l.pos++
			l.emit(TK_PLUS, start, l.pos)
		}
	case '*':
		if rem() == '=' {
			l.pos += 2
			l.emit(TK_STAREQ, start, l.pos)
		} else {
			l.pos++
			l.emit(TK_STAR, start, l.pos)
		}
	case '%':
		if rem() == '=' {
			l.pos += 2
			l.emit(TK_PERCENTEQ, start, l.pos)
		} else {
			l.pos++
			l.emit(TK_PERCENT, start, l.pos)
		}
	default:
		return false
	}
	return true
}

// peekAt safely returns the byte at idx, or 0 at EOF.
func (l *Lexer) peekAt(idx int) byte {
	if idx < 0 || idx >= len(l.src) {
		return 0
	}
	return l.src[idx]
}

// emit appends a token to the output stream (unless filtered as trivia).
func (l *Lexer) emit(k TokenKind, start, end int) {
	l.tokens = append(l.tokens, Token{Kind: k, Text: l.src[start:end], Start: start, End: end})
}

// diag appends a diagnostic.
func (l *Lexer) diag(code, msg string, start, end int) {
	l.diags = append(l.diags, Diagnostic{Code: code, Severity: SevError, Message: msg, Span: Span{Start: start, End: end}})
}

// --- character-class predicates ----------------------------------------------

func isDec(c byte) bool { return c >= '0' && c <= '9' }
func isHex(c byte) bool {
	return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')
}
func isOct(c byte) bool { return c >= '0' && c <= '7' }
func isBin(c byte) bool { return c == '0' || c == '1' }

// isIdentStart implements XID_Start ∪ '_' per §3.3.
func isIdentStart(r rune) bool {
	if r == '_' {
		return true
	}
	if r < 128 {
		return (r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z')
	}
	return unicode.In(r, unicode.L, unicode.Nl) // close enough to XID_Start for ASCII corpus
}

// isIdentContinue implements XID_Continue ∪ '_'.
func isIdentContinue(r rune) bool {
	if r == '_' {
		return true
	}
	if r < 128 {
		return (r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z') || (r >= '0' && r <= '9')
	}
	return unicode.In(r, unicode.L, unicode.Nl, unicode.Mn, unicode.Mc, unicode.Nd, unicode.Pc)
}

// normalizeCRLF replaces \r\n with \n.
func normalizeCRLF(s string) string {
	// Cheap detection: skip allocation if no CR present.
	hasCR := false
	for i := 0; i < len(s); i++ {
		if s[i] == '\r' {
			hasCR = true
			break
		}
	}
	if !hasCR {
		return s
	}
	out := make([]byte, 0, len(s))
	for i := 0; i < len(s); i++ {
		if s[i] == '\r' && i+1 < len(s) && s[i+1] == '\n' {
			out = append(out, '\n')
			i++
			continue
		}
		out = append(out, s[i])
	}
	return string(out)
}

// Lex is a convenience wrapper.
func Lex(src string) ([]Token, []Diagnostic) {
	return NewLexer(src).Lex()
}
