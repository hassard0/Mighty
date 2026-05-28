; Scope tracking for Mighty.
;
; Used by editors for go-to-definition, rename-in-scope, and unused-
; variable detection. We mark the major scope-creating constructs and
; the binding/reference sites.

; ---- scopes ------------------------------------------------------------
(source_file) @local.scope
(block) @local.scope
(function_declaration) @local.scope
(extern_function) @local.scope
(lambda_expression) @local.scope
(for_expression) @local.scope
(while_expression) @local.scope
(loop_expression) @local.scope
(if_expression) @local.scope
(match_expression) @local.scope
(match_arm) @local.scope
(agent_declaration) @local.scope
(on_handler) @local.scope
(impl_block) @local.scope
(trait_declaration) @local.scope
(budget_expression) @local.scope
(arena_expression) @local.scope
(unsafe_expression) @local.scope
(struct_declaration) @local.scope
(enum_declaration) @local.scope
(supervisor_declaration) @local.scope

; ---- definitions -------------------------------------------------------
(parameter name: (identifier) @local.definition.parameter)
(let_statement name: (identifier_pattern (identifier) @local.definition.var))
(function_declaration name: (identifier) @local.definition.function)
(agent_declaration name: (identifier) @local.definition.type)
(struct_declaration name: (identifier) @local.definition.type)
(enum_declaration name: (identifier) @local.definition.type)
(trait_declaration name: (identifier) @local.definition.type)
(type_alias name: (identifier) @local.definition.type)
(const_declaration name: (identifier) @local.definition.constant)
(generic_parameter (identifier) @local.definition.type)
(for_expression pattern: (identifier_pattern (identifier) @local.definition.var))
(on_handler message: (identifier) @local.definition.function)
(supervisor_declaration name: (identifier) @local.definition.type)
(sup_child (identifier) @local.definition.var)
(protocol_declaration name: (identifier) @local.definition.type)
(protocol_message name: (identifier) @local.definition.function)
(agent_state_field name: (identifier) @local.definition.field)
(struct_field name: (identifier) @local.definition.field)
(use_declaration (_) @local.definition.import)

; ---- references --------------------------------------------------------
(path_expression (identifier) @local.reference)
(path_type (identifier) @local.reference)
(field_expression field: (identifier) @local.reference)
(method_call_expression method: (identifier) @local.reference)
