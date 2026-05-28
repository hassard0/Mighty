; Syntax highlighting queries for Mighty.
;
; Capture vocabulary follows the nvim-treesitter convention so the same
; queries drop into Neovim, Helix, and Zed without translation.

; ---- comments ----------------------------------------------------------
(line_comment) @comment
(block_comment) @comment
(doc_comment) @comment.documentation

; ---- keywords ----------------------------------------------------------
; Anonymous-token keywords (tree-sitter recognises these as literal
; strings in the grammar; keywords used only inside a node value like
; `continue_expression` are highlighted via the node-name capture
; below).
[
  "agent"
  "arena"
  "as"
  "backoff"
  "child"
  "const"
  "derive"
  "detach"
  "dyn"
  "effect"
  "else"
  "enum"
  "export"
  "extern"
  "fn"
  "for"
  "if"
  "impl"
  "in"
  "let"
  "loop"
  "macro"
  "match"
  "mod"
  "move"
  "on"
  "on_fail"
  "package"
  "proc"
  "protocol"
  "requires"
  "restart"
  "return"
  "run"
  "sandbox"
  "spawn"
  "state"
  "struct"
  "sup"
  "supervisor"
  "trait"
  "type"
  "unsafe"
  "up_to"
  "use"
  "where"
  "while"
  "with"
  "yield"
] @keyword

; Keywords inside specific node types
(continue_expression) @keyword.return
(break_expression "break" @keyword.return)
(return_expression "return" @keyword.return)
(yield_expression "yield" @keyword.return)


; "budget" is a soft keyword — only highlight it as such inside a
; budget_expression context.
(budget_expression "budget" @keyword)
(budget_expression "run" @keyword)

; ---- operators ---------------------------------------------------------
[
  "+" "-" "*" "/" "%"
  "==" "!=" "<" ">" "<=" ">="
  "&&" "||" "!"
  "&" "|" "^" "<<" ">>"
  "=" "+=" "-=" "*=" "/=" "%=" "&=" "|=" "^=" "<<=" ">>="
  "->" "=>"
  ".." "..="
] @operator

; ---- punctuation -------------------------------------------------------
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," ";" ":" "::" "."] @punctuation.delimiter

; Agent-message markers — Mighty's signature send/ask syntax.
(send_expression "!" @operator.special)
(ask_expression  "?" @operator.special)

; Postfix `?` (Result propagation)
(question_expression "?" @operator.special)

; Deadline marker
(deadline_expression "@" @operator.special)

; ---- literals ----------------------------------------------------------
(integer_literal)  @number
(float_literal)    @number.float
(duration_literal) @number
(size_literal)     @number
(string_literal)   @string
(html_literal)     @string.special
(escape_sequence)  @string.escape
(string_interpolation "{" @punctuation.special)
(string_interpolation "}" @punctuation.special)
(char_literal)     @character
(boolean_literal)  @boolean
(unit_literal)     @constant.builtin
(null_literal)     @constant.builtin

; ---- attributes & macros ----------------------------------------------
(attribute_at "@" @attribute)
(attribute_at name: (identifier) @attribute)
(attribute_hash) @attribute
(derive_attribute "derive" @attribute)

(macro_invocation name: (identifier) @function.macro)

(macro_declaration name: (identifier) @function.macro)
(proc_macro_declaration name: (identifier) @function.macro)

; ---- functions ---------------------------------------------------------
(function_declaration name: (identifier) @function)
(extern_function name: (identifier) @function)
(export_declaration name: (identifier) @function)

(call_expression
  function: (path_expression (identifier) @function.call .))

(method_call_expression method: (identifier) @function.method)

; ---- agents / protocols / supervisors ---------------------------------
(agent_declaration name: (identifier) @type)
(protocol_declaration name: (identifier) @type)
(supervisor_declaration name: (identifier) @type)
(protocol_message name: (identifier) @function)
(on_handler message: (identifier) @function)

; ---- types -------------------------------------------------------------
(struct_declaration name: (identifier) @type)
(enum_declaration name: (identifier) @type)
(trait_declaration name: (identifier) @type)
(type_alias name: (identifier) @type)
(enum_variant name: (identifier) @constructor)

; Capital-case identifiers in type position are types
(path_type (identifier) @type
  (#match? @type "^[A-Z]"))

; Tainted[T] — the marketing-signal type. Highlight the wrapper as
; @type.tainted so editor themes can show it distinctly. (Falls back to
; @type if a theme doesn't define a tainted variant.)
((path_type (identifier) @type.builtin.tainted)
  (#eq? @type.builtin.tainted "Tainted"))

; Builtin numeric / string / collection types
((path_type (identifier) @type.builtin)
  (#match? @type.builtin "^(I8|I16|I32|I64|I128|U8|U16|U32|U64|U128|USize|F32|F64|Bool|Str|String|Char|Unit|Bytes|Option|Result|Vec|Map|Set)$"))

; ---- patterns / bindings ----------------------------------------------
(parameter name: (identifier) @variable.parameter)
(let_statement name: (identifier_pattern (identifier) @variable))
(struct_field name: (identifier) @property)
(field_expression field: (identifier) @property)

(named_argument name: (identifier) @variable.parameter)

(wildcard_pattern) @variable.builtin

; ---- generics ----------------------------------------------------------
(generic_parameter (identifier) @type.parameter)

; ---- effect rows -------------------------------------------------------
(effect_clause_kw (identifier) @attribute)

; ---- visibility --------------------------------------------------------
(visibility) @keyword.modifier

; ---- paths -------------------------------------------------------------
(path_expression (identifier) @variable)

; std.* / package paths — first ident often a namespace
((path_expression
   (identifier) @namespace
   .)
  (#match? @namespace "^(std|self|super|crate)$"))

; ---- self --------------------------------------------------------------
((identifier) @variable.builtin
 (#eq? @variable.builtin "self"))
