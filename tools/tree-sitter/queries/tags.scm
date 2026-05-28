; Symbol extraction (ctags-style) for Mighty.
;
; Consumed by GitHub linguist + JetBrains structure view + Zed outline +
; nvim-treesitter "list symbols" pickers.

; ---- definitions -------------------------------------------------------
(function_declaration
  name: (identifier) @name) @definition.function

(extern_function
  name: (identifier) @name) @definition.function

(struct_declaration
  name: (identifier) @name) @definition.struct

(enum_declaration
  name: (identifier) @name) @definition.enum

(trait_declaration
  name: (identifier) @name) @definition.interface

(type_alias
  name: (identifier) @name) @definition.type

(const_declaration
  name: (identifier) @name) @definition.constant

(agent_declaration
  name: (identifier) @name) @definition.class

(protocol_declaration
  name: (identifier) @name) @definition.interface

(supervisor_declaration
  name: (identifier) @name) @definition.class

(on_handler
  message: (identifier) @name) @definition.method

(protocol_message
  name: (identifier) @name) @definition.method

(impl_block) @definition.implementation

(macro_declaration
  name: (identifier) @name) @definition.macro

(proc_macro_declaration
  name: (identifier) @name) @definition.macro

(module_declaration
  (identifier) @name) @definition.module

(package_declaration
  (_) @name) @definition.module

; ---- references --------------------------------------------------------
(call_expression
  function: (path_expression (identifier) @name)) @reference.call

(method_call_expression
  method: (identifier) @name) @reference.call

(macro_invocation
  name: (identifier) @name) @reference.call

(send_expression
  message: (identifier) @name) @reference.call

(ask_expression
  message: (identifier) @name) @reference.call
