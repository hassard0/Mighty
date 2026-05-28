/**
 * tree-sitter-mighty — grammar for the Mighty agent-first language
 *
 * Spec: docs/spec/v1.0-rc.md (alongside this file in the Mighty repo)
 * Canonical parser: crates/mty-syntax/src/parser/
 *
 * This grammar covers v0.30 surface syntax (the v0.31 cut). It is
 * permissive on purpose — the goal is best-effort highlighting + symbol
 * extraction across IDEs (Neovim, Helix, Zed, plus VS Code + JetBrains
 * plugins that consume the tree-sitter queries). The canonical parser
 * in crates/mty-syntax remains the source of truth for compilation
 * diagnostics; tree-sitter is for editor experience only.
 *
 * Design notes:
 *   - `budget` is a soft keyword (per v0.29 Track E). We model it as a
 *     contextual block expression that only triggers when followed by
 *     `{ entries... } run { ... }`.
 *   - Effect rows `!{a, b | E}` are parsed as a postfix on type
 *     references. The legacy `effect a, b` keyword form is also
 *     supported in fn signature position.
 *   - Generics use `[T]` (square brackets) not `<T>`.
 *   - `Tainted[T]` is just a generic type path — but we expose it as a
 *     named type via a query so editors can colour it distinctly.
 *   - Macros: `name!(args)` and the declarative `macro name(a, b) => { ... }`
 *     form plus the procedural `proc macro name(...) -> ... { ... }`.
 *   - Attributes: `@tool(...)` and `@computer_use(...)` decorators precede
 *     fn/agent/protocol items; `#[derive(...)]` also supported.
 */

const PREC = {
  unary: 14,
  cast: 13,
  multiplicative: 12,
  additive: 11,
  shift: 10,
  bitand: 9,
  bitxor: 8,
  bitor: 7,
  comparative: 6,
  and: 5,
  or: 4,
  range: 3,
  assign: 2,
  closure: 1,
};

module.exports = grammar({
  name: 'mighty',

  extras: $ => [
    /\s+/,
    $.line_comment,
    $.block_comment,
    $.doc_comment,
  ],

  word: $ => $.identifier,

  conflicts: $ => [
    [$.generic_parameter, $._path],
    [$._path, $.path_expression],
    [$._type, $.result_sugar_type, $.effect_row_type],
    [$._path, $.identifier_pattern],
    [$.identifier_pattern, $.path_expression],
    [$.identifier_pattern, $.struct_expression_field],
  ],

  rules: {
    // ---------------------------------------------------------------- file
    source_file: $ => repeat($._item),

    // ---------------------------------------------------------------- trivia
    line_comment: _ => token(seq('//', /[^\n]*/)),
    block_comment: _ => token(seq('/*', /[^*]*\*+([^/*][^*]*\*+)*/, '/')),
    doc_comment: _ => token(seq('///', /[^\n]*/)),

    // ---------------------------------------------------------------- items
    _item: $ => choice(
      $.use_declaration,
      $.package_declaration,
      $.module_declaration,
      $.function_declaration,
      $.struct_declaration,
      $.enum_declaration,
      $.type_alias,
      $.const_declaration,
      $.impl_block,
      $.trait_declaration,
      $.agent_declaration,
      $.protocol_declaration,
      $.supervisor_declaration,
      $.extern_block,
      $.export_declaration,
      $.macro_declaration,
      $.proc_macro_declaration,
      $.sandbox_declaration,
    ),

    // ---- attributes (v0.27 @tool / v0.30 @computer_use, plus #[derive(...)])
    attribute_at: $ => seq(
      '@',
      field('name', $.identifier),
      '(',
      optional(commaSep1(choice(
        $.named_argument,
        $._expression,
      ))),
      ')',
    ),

    attribute_hash: $ => seq(
      '#',
      '[',
      field('name', $.identifier),
      optional(seq('(', commaSep($.identifier), ')')),
      ']',
    ),

    derive_attribute: $ => seq(
      'derive',
      commaSep1($.identifier),
    ),

    _attribute: $ => choice(
      $.attribute_at,
      $.attribute_hash,
      $.derive_attribute,
    ),

    visibility: _ => 'pub',

    // ---- use / package / mod
    use_declaration: $ => seq(
      'use',
      $._path,
      optional(seq('as', $.identifier)),
      optional(';'),
    ),

    package_declaration: $ => seq(
      'package',
      $._path,
      optional(';'),
    ),

    module_declaration: $ => seq(
      'mod',
      $.identifier,
      choice(';', seq('{', repeat($._item), '}')),
    ),

    // ---- function declaration
    function_declaration: $ => prec.right(seq(
      repeat($._attribute),
      optional($.visibility),
      optional('unsafe'),
      'fn',
      field('name', $.identifier),
      optional($.generic_parameters),
      $.parameters,
      optional(seq('->', field('return_type', $._type))),
      optional($.effect_clause_kw),
      optional($.where_clause),
      optional(choice(
        seq('=', $._expression),  // export c fn foo() = expr
        repeat1($.requires_clause),
        $.block,
        ';',
      )),
    )),

    parameters: $ => seq(
      '(',
      optional(seq($.parameter, repeat(seq(',', $.parameter)), optional(','))),
      ')',
    ),

    parameter: $ => seq(
      optional('mut'),
      field('name', $.identifier),
      optional(seq(':', field('type', $._type))),
    ),

    generic_parameters: $ => seq(
      '[',
      commaSep1($.generic_parameter),
      ']',
    ),

    generic_parameter: $ => seq(
      $.identifier,
      optional(seq(':', $._type)),
    ),

    where_clause: $ => seq(
      'where',
      commaSep1($.where_predicate),
    ),

    where_predicate: $ => seq(
      $._type,
      ':',
      sepBy1('+', $._type),
    ),

    requires_clause: $ => seq(
      'requires',
      $._expression,
    ),

    // ---- struct
    struct_declaration: $ => seq(
      repeat($._attribute),
      optional($.visibility),
      'struct',
      field('name', $.identifier),
      optional($.generic_parameters),
      choice(
        seq('{', repeat(seq($.struct_field, optional(','))), '}'),
        seq('(', optional(commaSep1($._type)), ')', optional(';')),
        ';',
      ),
    ),

    struct_field: $ => seq(
      repeat($._attribute),
      optional($.visibility),
      field('name', $.identifier),
      ':',
      field('type', $._type),
    ),

    // ---- enum
    enum_declaration: $ => seq(
      repeat($._attribute),
      optional($.visibility),
      'enum',
      field('name', $.identifier),
      optional($.generic_parameters),
      '{',
      repeat(seq($.enum_variant, optional(','))),
      '}',
    ),

    enum_variant: $ => seq(
      field('name', $.identifier),
      optional(choice(
        seq('(', optional(commaSep1($._type)), ')'),
        seq('{', optional(commaSep1Trailing($.struct_field)), '}'),
      )),
    ),

    // ---- type alias
    type_alias: $ => seq(
      optional($.visibility),
      'type',
      field('name', $.identifier),
      optional($.generic_parameters),
      '=',
      $._type,
      optional(';'),
    ),

    // ---- const
    const_declaration: $ => seq(
      optional($.visibility),
      'const',
      field('name', $.identifier),
      ':',
      $._type,
      '=',
      $._expression,
      optional(';'),
    ),

    // ---- impl
    impl_block: $ => seq(
      'impl',
      optional($.generic_parameters),
      $._type,
      optional(seq('for', $._type)),
      '{',
      repeat($._item),
      '}',
    ),

    // ---- trait
    trait_declaration: $ => seq(
      optional($.visibility),
      'trait',
      field('name', $.identifier),
      optional($.generic_parameters),
      '{',
      repeat($._item),
      '}',
    ),

    // ---- agent
    agent_declaration: $ => seq(
      repeat($._attribute),
      optional($.visibility),
      'agent',
      field('name', $.identifier),
      optional($.agent_ctor_params),
      optional(seq(':', sepBy1('+', $._type))),
      '{',
      repeat($._agent_member),
      '}',
    ),

    agent_ctor_params: $ => seq(
      '(',
      optional(commaSep1($.identifier)),
      ')',
    ),

    _agent_member: $ => choice(
      $.on_handler,
      $.agent_state_field,
      $.function_declaration,
    ),

    agent_state_field: $ => seq(
      optional('state'),
      field('name', $.identifier),
      optional(seq(':', field('type', $._type))),
      optional(seq('=', field('value', $._expression))),
      optional(';'),
    ),

    on_handler: $ => seq(
      'on',
      field('message', $.identifier),
      optional(seq(
        '(',
        optional(commaSep1($.identifier)),
        ')',
      )),
      choice(
        seq('->', $._expression),
        $.block,
      ),
    ),

    // ---- protocol
    protocol_declaration: $ => seq(
      optional($.visibility),
      'protocol',
      field('name', $.identifier),
      '{',
      repeat($.protocol_message),
      '}',
    ),

    protocol_message: $ => seq(
      field('name', $.identifier),
      '(',
      optional(commaSep1($.parameter)),
      ')',
      optional(seq('->', $._type)),
      optional($.effect_clause_kw),
      optional(';'),
    ),

    // ---- supervisor
    supervisor_declaration: $ => seq(
      choice('sup', 'supervisor'),
      field('name', $.identifier),
      optional(seq('(', commaSep($.named_argument), ')')),
      '{',
      repeat($._supervisor_member),
      '}',
    ),

    _supervisor_member: $ => choice(
      $.sup_child,
      $.on_fail_clause,
    ),

    sup_child: $ => seq(
      'child',
      $.identifier,
      '=',
      $._expression,
      optional(';'),
    ),

    on_fail_clause: $ => seq(
      'on_fail',
      '(',
      $.identifier,
      ')',
      '{',
      repeat($._fail_action),
      '}',
    ),

    _fail_action: $ => seq(
      choice(
        seq('restart', optional(seq('up_to', $.integer_literal, 'in', $.duration_literal))),
        seq('backoff', $._expression),
        'detach',
      ),
      optional(';'),
    ),

    // ---- extern (extern { ... } / extern c { ... } / extern js { ... })
    extern_block: $ => seq(
      'extern',
      optional(choice('c', $.string_literal)),
      optional(choice('c', 'js', $.identifier)),
      '{',
      repeat($.extern_function),
      '}',
    ),

    extern_function: $ => seq(
      'fn',
      field('name', $.identifier),
      $.parameters,
      optional(seq('->', $._type)),
      optional($.effect_clause_kw),
      optional(';'),
    ),

    // ---- export
    export_declaration: $ => seq(
      'export',
      optional(choice('c', 'js', $.identifier)),
      'fn',
      field('name', $.identifier),
      $.parameters,
      optional(seq('->', $._type)),
      choice(
        seq('=', $._expression),
        $.block,
      ),
    ),

    // ---- macros
    macro_declaration: $ => seq(
      'macro',
      field('name', $.identifier),
      '(',
      optional(commaSep1($.identifier)),
      ')',
      '=>',
      $.token_tree,
    ),

    proc_macro_declaration: $ => seq(
      'proc',
      'macro',
      field('name', $.identifier),
      $.parameters,
      optional(seq('->', $._type)),
      $.block,
    ),

    token_tree: $ => choice(
      seq('{', repeat($._token_in_tree), '}'),
      seq('(', repeat($._token_in_tree), ')'),
      seq('[', repeat($._token_in_tree), ']'),
    ),

    _token_in_tree: $ => choice(
      $.token_tree,
      $._literal,
      $.identifier,
      /[^(){}\[\]\s]+/,
    ),

    // ---- sandbox (top-level form)
    sandbox_declaration: $ => seq(
      'sandbox',
      field('name', $.identifier),
      optional(seq('with', '{', repeat($.sandbox_entry), '}')),
      $.block,
    ),

    sandbox_entry: $ => seq(
      sepBy1('.', $.identifier),
      '=',
      $._expression,
      optional(';'),
    ),

    // ---------------------------------------------------------------- types
    _type: $ => choice(
      $.result_sugar_type,
      $.effect_row_type,
      $._type_no_suffix,
    ),

    _type_no_suffix: $ => choice(
      $.path_type,
      $.tuple_type,
      $.array_type,
      $.borrow_type,
      $.pointer_type,
      $.function_type,
      $.unit_type,
      $.dyn_type,
    ),

    pointer_type: $ => seq('*', optional('mut'), $._type_no_suffix),

    // `T!Err` — anonymous Result sugar (errors are concrete type).
    result_sugar_type: $ => prec.left(seq(
      $._type_no_suffix,
      '!',
      $._type_no_suffix,
    )),

    // `T!{a, b | E}` — effect-row clause.
    effect_row_type: $ => seq(
      $._type_no_suffix,
      '!',
      '{',
      optional(seq(
        commaSep1($.identifier),
      )),
      optional(seq('|', commaSep1($.identifier))),
      '}',
    ),

    path_type: $ => prec.left(seq(
      $._path,
      optional($.generic_arguments_type),
    )),

    generic_arguments_type: $ => seq(
      '[',
      commaSep1($._type),
      ']',
    ),

    tuple_type: $ => seq(
      '(',
      $._type,
      ',',
      optional(commaSep1($._type)),
      ')',
    ),

    array_type: $ => seq(
      '[',
      $._type,
      optional(seq(';', $._expression)),
      ']',
    ),

    borrow_type: $ => seq(
      '&',
      optional('mut'),
      $._type,
    ),

    function_type: $ => seq(
      'fn',
      '(',
      optional(commaSep1($._type)),
      ')',
      optional(seq('->', $._type)),
    ),

    unit_type: _ => seq('(', ')'),

    dyn_type: $ => seq('dyn', $._type),

    // ---- effect rows
    //
    // Two forms:
    //   * `!{a, b | E}` postfix on a return type (parsed by the type parser)
    //   * legacy `effect a, b | E` keyword form (after the return type)
    effect_clause_kw: $ => seq(
      'effect',
      commaSep1($.identifier),
      optional(seq('|', commaSep1($.identifier))),
    ),

    // ---------------------------------------------------------------- paths
    _path: $ => prec.left(seq(
      $.identifier,
      repeat(seq(choice('.', '::'), $.identifier)),
    )),

    // ---------------------------------------------------------------- statements / block
    block: $ => seq(
      '{',
      repeat($._statement),
      '}',
    ),

    _statement: $ => choice(
      $.let_statement,
      $.expression_statement,
      $._item,
    ),

    let_statement: $ => seq(
      'let',
      optional('mut'),
      field('name', $._pattern),
      optional(seq(':', field('type', $._type))),
      optional(seq('=', field('value', $._expression))),
      optional(';'),
    ),

    expression_statement: $ => choice(
      seq($._expression, ';'),
      $._expression,
    ),

    // ---------------------------------------------------------------- patterns
    _pattern: $ => choice(
      $.identifier_pattern,
      $.wildcard_pattern,
      $.tuple_pattern,
      $.struct_pattern,
      $.enum_pattern,
      $.literal_pattern,
      $.ref_pattern,
    ),

    identifier_pattern: $ => $.identifier,

    wildcard_pattern: _ => '_',

    tuple_pattern: $ => seq('(', commaSep($._pattern), ')'),

    struct_pattern: $ => seq(
      $._path,
      '{',
      commaSep($._pattern),
      optional(seq(',', '..')),
      '}',
    ),

    enum_pattern: $ => prec.left(1, choice(
      // Path with multiple segments (Shape.Circle), optionally with args
      seq(
        $.identifier,
        repeat1(seq(choice('.', '::'), $.identifier)),
        optional(seq('(', commaSep($._pattern), ')')),
      ),
      // Single identifier MUST be followed by args (otherwise it's
      // identifier_pattern)
      seq(
        $.identifier,
        '(',
        commaSep($._pattern),
        ')',
      ),
    )),

    literal_pattern: $ => prec(1, $._literal),

    range_pattern: $ => prec.left(seq(
      $._expression,
      choice('..', '..='),
      $._expression,
    )),

    ref_pattern: $ => seq('&', optional('mut'), $._pattern),

    // ---------------------------------------------------------------- expressions
    _expression: $ => choice(
      $._literal,
      $.path_expression,
      $.binary_expression,
      $.unary_expression,
      $.call_expression,
      $.method_call_expression,
      $.field_expression,
      $.index_expression,
      $.assignment_expression,
      $.compound_assignment_expression,
      $.cast_expression,
      $.if_expression,
      $.match_expression,
      $.for_expression,
      $.while_expression,
      $.loop_expression,
      $.return_expression,
      $.break_expression,
      $.continue_expression,
      $.yield_expression,
      $.tuple_expression,
      $.array_expression,
      $.struct_expression,
      $.map_expression,
      $.lambda_expression,
      $.send_expression,
      $.ask_expression,
      $.deadline_expression,
      $.question_expression,
      $.move_expression,
      $.borrow_expression,
      $.spawn_expression,
      $.run_expression,
      $.unsafe_expression,
      $.parenthesized_expression,
      $.macro_invocation,
      $.budget_expression,
      $.arena_expression,
      $.block,
      $.range_expression,
    ),

    parenthesized_expression: $ => seq('(', $._expression, ')'),

    path_expression: $ => prec.left(seq(
      $.identifier,
      repeat(seq(choice('.', '::'), $.identifier)),
    )),

    binary_expression: $ => {
      const table = [
        [PREC.or,             '||'],
        [PREC.and,            '&&'],
        [PREC.comparative,    choice('==', '!=', '<', '>', '<=', '>=')],
        [PREC.bitor,          '|'],
        [PREC.bitxor,         '^'],
        [PREC.bitand,         '&'],
        [PREC.shift,          choice('<<', '>>')],
        [PREC.additive,       choice('+', '-')],
        [PREC.multiplicative, choice('*', '/', '%')],
      ];
      return choice(...table.map(([precedence, operator]) =>
        prec.left(precedence, seq(
          field('left', $._expression),
          field('operator', operator),
          field('right', $._expression),
        )),
      ));
    },

    unary_expression: $ => prec(PREC.unary, seq(
      choice('-', '!', '*'),
      $._expression,
    )),

    call_expression: $ => prec(15, seq(
      field('function', $._expression),
      field('arguments', $.argument_list),
    )),

    argument_list: $ => seq(
      '(',
      optional(seq(
        choice($.named_argument, $._expression),
        repeat(seq(',', choice($.named_argument, $._expression))),
        optional(','),
      )),
      ')',
    ),

    named_argument: $ => seq(
      field('name', $.identifier),
      ':',
      field('value', $._expression),
    ),

    method_call_expression: $ => prec(15, seq(
      field('receiver', $._expression),
      '.',
      field('method', $.identifier),
      optional($.generic_arguments_type),
      field('arguments', $.argument_list),
    )),

    field_expression: $ => prec(14, seq(
      field('receiver', $._expression),
      '.',
      field('field', choice($.identifier, $.integer_literal)),
    )),

    index_expression: $ => prec(15, seq(
      field('receiver', $._expression),
      '[',
      field('index', $._expression),
      ']',
    )),

    assignment_expression: $ => prec.right(PREC.assign, seq(
      field('left', $._expression),
      '=',
      field('right', $._expression),
    )),

    compound_assignment_expression: $ => prec.right(PREC.assign, seq(
      field('left', $._expression),
      field('operator', choice('+=', '-=', '*=', '/=', '%=', '&=', '|=', '^=', '<<=', '>>=')),
      field('right', $._expression),
    )),

    cast_expression: $ => prec.left(PREC.cast, seq(
      $._expression,
      'as',
      $._type,
    )),

    range_expression: $ => prec.left(PREC.range, choice(
      seq($._expression, choice('..', '..='), $._expression),
      seq($._expression, '..'),
      seq('..', $._expression),
      '..',
    )),

    if_expression: $ => seq(
      'if',
      choice(
        $._expression,
        seq('let', $._pattern, '=', $._expression),
      ),
      field('consequence', $.block),
      optional(seq('else', field('alternative', choice($.if_expression, $.block)))),
    ),

    match_expression: $ => seq(
      'match',
      field('scrutinee', $._expression),
      '{',
      repeat($.match_arm),
      '}',
    ),

    match_arm: $ => prec.right(seq(
      field('pattern', $._match_pattern),
      optional($.match_guard),
      '=>',
      field('value', $._expression),
      optional(','),
    )),

    _match_pattern: $ => choice(
      $._pattern,
      $.range_pattern,
      $.or_pattern,
    ),

    or_pattern: $ => prec.left(seq($._pattern, repeat1(seq('|', $._pattern)))),

    match_guard: $ => seq('if', $._expression),

    for_expression: $ => seq(
      'for',
      field('pattern', $._pattern),
      'in',
      field('value', $._expression),
      field('body', $.block),
    ),

    while_expression: $ => seq(
      'while',
      choice(
        $._expression,
        seq('let', $._pattern, '=', $._expression),
      ),
      field('body', $.block),
    ),

    loop_expression: $ => seq('loop', $.block),

    return_expression: $ => prec.right(seq('return', optional($._expression))),
    break_expression: $ => prec.right(seq('break', optional($._expression))),
    continue_expression: _ => 'continue',
    yield_expression: $ => prec.right(seq('yield', optional($._expression))),

    tuple_expression: $ => seq(
      '(',
      $._expression,
      ',',
      optional(commaSep1($._expression)),
      ')',
    ),

    array_expression: $ => seq(
      '[',
      optional(choice(
        seq($._expression, ';', $._expression),
        commaSep1Trailing($._expression),
      )),
      ']',
    ),

    struct_expression: $ => prec(1, seq(
      field('name', $._path),
      optional($.generic_arguments_type),
      '{',
      commaSep($.struct_expression_field),
      '}',
    )),

    struct_expression_field: $ => choice(
      seq(field('name', $.identifier), ':', field('value', $._expression)),
      $.identifier,
    ),

    // Map literal: `Map::[K, V]{}` or `{key: value, ...}`
    map_expression: $ => seq(
      $._path,
      '::',
      $.generic_arguments_type,
      '{',
      commaSep($.map_entry),
      '}',
    ),

    map_entry: $ => seq($._expression, ':', $._expression),

    lambda_expression: $ => prec.right(PREC.closure, seq(
      'fn',
      '(',
      optional(commaSep1($.parameter)),
      ')',
      optional(seq('->', $._type)),
      choice($.block, $._expression),
    )),

    // ---- agent send/ask: `agent!Msg(args)` and `agent?Msg(args)`
    //
    // Uses dynamic prec so the GLR parser explores both send_expression
    // and macro_invocation when it sees `IDENT !`, and picks based on
    // what follows the `!`. dynamic(1) vs no dynamic for the other rule
    // means send wins ties — but in practice the disambiguator (IDENT
    // vs `(`/`{`/`[`) decides.
    send_expression: $ => prec.dynamic(1, prec.left(15, seq(
      field('receiver', $._expression),
      '!',
      field('message', $.identifier),
      optional(field('arguments', $.argument_list)),
    ))),

    ask_expression: $ => prec.dynamic(1, prec.left(15, seq(
      field('receiver', $._expression),
      '?',
      field('message', $.identifier),
      optional(field('arguments', $.argument_list)),
    ))),

    // `expr @2s` deadline-bound expression
    deadline_expression: $ => prec.left(13, seq(
      $._expression,
      '@',
      $.duration_literal,
    )),

    // Postfix `?` for Result propagation.
    question_expression: $ => prec(14, seq(
      $._expression,
      '?',
    )),

    move_expression: $ => prec.right(PREC.unary, seq('move', $._expression)),

    borrow_expression: $ => prec.right(PREC.unary, seq(
      '&',
      optional('mut'),
      $._expression,
    )),

    spawn_expression: $ => prec.right(seq('spawn', $._expression)),
    run_expression: $ => prec.right(seq('run', $._expression)),

    unsafe_expression: $ => seq('unsafe', $.block),

    // ---- budget { ... } run { ... }  (soft kw `budget`)
    budget_expression: $ => seq(
      'budget',
      '{',
      repeat($.budget_entry),
      '}',
      'run',
      $.block,
    ),

    budget_entry: $ => seq(
      $.identifier,
      $._expression,
      optional(';'),
    ),

    // ---- arena name { ... } / arena name: expr
    arena_expression: $ => seq(
      'arena',
      field('name', $.identifier),
      choice(
        $.block,
        seq(':', $._expression),
      ),
    ),

    // ---- macro invocation: `name!(args)` or `name!{tokens}` etc.
    //
    // The `!` plus token-tree opener is modeled as a single immediate
    // token (`!(`, `!{`, `![`) so the lexer commits to macro syntax only
    // when the opener follows directly. This distinguishes from
    // send_expression (`recv!Msg(...)` — `!` followed by IDENT).
    //
    // The opener is part of the bang token, so the token_tree body
    // matches the content + matching close brace.
    macro_invocation: $ => choice(
      seq(field('name', $.identifier), $._bang_lparen, repeat($._token_in_tree), ')'),
      seq(field('name', $.identifier), $._bang_lbrace, repeat($._token_in_tree), '}'),
      seq(field('name', $.identifier), $._bang_lbrack, repeat($._token_in_tree), ']'),
    ),

    _bang_lparen: _ => token.immediate('!('),
    _bang_lbrace: _ => token.immediate('!{'),
    _bang_lbrack: _ => token.immediate('!['),

    // ---------------------------------------------------------------- literals
    _literal: $ => choice(
      $.integer_literal,
      $.float_literal,
      $.duration_literal,
      $.size_literal,
      $.string_literal,
      $.html_literal,
      $.char_literal,
      $.boolean_literal,
      $.unit_literal,
      $.null_literal,
    ),

    integer_literal: _ => token(seq(
      /[0-9][0-9_]*/,
      optional(/[iuf](?:8|16|32|64|128)/),
    )),

    float_literal: _ => token(seq(
      /[0-9][0-9_]*\.[0-9][0-9_]*/,
      optional(/f(?:32|64)/),
    )),

    duration_literal: _ => token(seq(/[0-9]+/, /(?:ns|us|ms|s|m|h)/)),

    size_literal: _ => token(seq(/[0-9]+/, /(?:KiB|MiB|GiB|B|k|M)/)),

    string_literal: $ => seq(
      '"',
      repeat(choice(
        $.escape_sequence,
        $.string_interpolation,
        /[^"\\{]+/,
        /\{[^}{]*\}/,
      )),
      '"',
    ),

    html_literal: _ => token(seq('html"', /([^"\\]|\\.)*/, '"')),

    string_interpolation: $ => seq('{', $._expression, '}'),

    escape_sequence: _ => token(seq('\\', /./)),

    char_literal: _ => token(seq("'", choice(seq('\\', /./), /[^'\\]/), "'")),

    boolean_literal: _ => choice('true', 'false'),

    unit_literal: _ => prec(2, seq('(', ')')),

    null_literal: _ => 'null',

    // ---------------------------------------------------------------- identifiers
    identifier: _ => /[A-Za-z_][A-Za-z0-9_]*/,
  },
});

// ---- helpers ---------------------------------------------------------
function commaSep(rule) {
  return optional(commaSep1(rule));
}
function commaSep1(rule) {
  return seq(rule, repeat(seq(',', rule)));
}
function commaSep1Trailing(rule) {
  return seq(rule, repeat(seq(',', rule)), optional(','));
}
function sepBy1(sep, rule) {
  return seq(rule, repeat(seq(sep, rule)));
}
