; Embedded-language injections for Mighty.
;
; Recognises:
;   - html"..." literals as HTML
;   - extern js { ... } blocks as JavaScript surface (only the fn sigs
;     parse here, the body is on the JS host side, so this is symbolic).
;   - Inline `// LANG: <lang>` line comments preceding a string literal
;     hint the embedded language for the following string.
;   - Doc comments as markdown.

((doc_comment) @injection.content
  (#set! injection.language "markdown"))

((html_literal) @injection.content
  (#set! injection.language "html"))

; format!("...{}...") strings — treat the format-spec as a tiny DSL.
((macro_invocation
   name: (identifier) @_name
   (token_tree
     (string_literal) @injection.content))
  (#eq? @_name "format")
  (#set! injection.language "mighty-format"))

; SQL injection via well-known sql! macro.
((macro_invocation
   name: (identifier) @_name
   (token_tree
     (string_literal) @injection.content))
  (#eq? @_name "sql")
  (#set! injection.language "sql"))

; Regex literal via re! macro.
((macro_invocation
   name: (identifier) @_name
   (token_tree
     (string_literal) @injection.content))
  (#eq? @_name "re")
  (#set! injection.language "regex"))
