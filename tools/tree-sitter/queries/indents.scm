; Indent rules for Mighty.
;
; Follows the nvim-treesitter @indent.begin / @indent.end convention.
; Editors (Helix, Zed) consume the same captures.

[
  (block)
  (struct_declaration)
  (enum_declaration)
  (agent_declaration)
  (protocol_declaration)
  (supervisor_declaration)
  (impl_block)
  (trait_declaration)
  (extern_block)
  (match_expression)
  (token_tree)
  (argument_list)
  (parameters)
  (generic_parameters)
  (array_expression)
  (tuple_expression)
  (struct_expression)
  (map_expression)
  (budget_expression)
  (arena_expression)
  (sandbox_declaration)
  (sandbox_entry)
  (on_handler)
  (on_fail_clause)
] @indent.begin

[
  "}"
  "]"
  ")"
] @indent.branch @indent.end

; Hanging operators kept at same level.
(binary_expression) @indent.align
