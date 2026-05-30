#[allow(unused_imports)]
use crate::{ast_node, AstNode};
use mty_syntax::{SyntaxKind, SyntaxNode};

ast_node!(File, FILE);
ast_node!(PackageDecl, PACKAGE_DECL);
ast_node!(UseDecl, USE_DECL);
ast_node!(ModDecl, MOD_DECL);
ast_node!(FnDecl, FN_DECL);
ast_node!(FnParamList, FN_PARAM_LIST);
ast_node!(FnParam, FN_PARAM);
ast_node!(RetType, RET_TYPE);
ast_node!(EffectClause, EFFECT_CLAUSE);
ast_node!(StructDecl, STRUCT_DECL);
ast_node!(StructField, STRUCT_FIELD);
ast_node!(EnumDecl, ENUM_DECL);
ast_node!(EnumVariant, ENUM_VARIANT);
ast_node!(TypeAlias, TYPE_ALIAS);
ast_node!(ImplBlock, IMPL_BLOCK);
ast_node!(TraitDecl, TRAIT_DECL);
ast_node!(TraitMethod, TRAIT_METHOD);
ast_node!(AgentDecl, AGENT_DECL);
ast_node!(AgentCtorParams, AGENT_CTOR_PARAMS);
ast_node!(AgentProtocolList, AGENT_PROTOCOL_LIST);
ast_node!(AgentStateDecl, AGENT_STATE_DECL);
ast_node!(OnHandler, ON_HANDLER);
ast_node!(ProtocolDecl, PROTOCOL_DECL);
ast_node!(ProtocolMsg, PROTOCOL_MSG);
ast_node!(SupervisorDecl, SUPERVISOR_DECL);
ast_node!(SupChild, SUP_CHILD);
ast_node!(OnFailClause, ON_FAIL_CLAUSE);
ast_node!(BudgetBlock, BUDGET_BLOCK);
ast_node!(BudgetEntry, BUDGET_ENTRY);
ast_node!(SandboxBlock, SANDBOX_BLOCK);
ast_node!(SandboxEntry, SANDBOX_ENTRY);
ast_node!(ArenaBlock, ARENA_BLOCK);
ast_node!(TaskScope, TASK_SCOPE);
ast_node!(ExternBlock, EXTERN_BLOCK);
ast_node!(ExternFn, EXTERN_FN);
ast_node!(ExportDecl, EXPORT_DECL);
ast_node!(MacroDecl, MACRO_DECL);
ast_node!(ConstDecl, CONST_DECL);
ast_node!(UnsafeBlock, UNSAFE_BLOCK);
ast_node!(Block, BLOCK);
ast_node!(LetStmt, LET_STMT);
ast_node!(ExprStmt, EXPR_STMT);
// Path/Name
ast_node!(Path, PATH);
ast_node!(PathSegment, PATH_SEGMENT);
ast_node!(Name, NAME);
ast_node!(NameRef, NAME_REF);
// Generic + Visibility + Attr
ast_node!(GenericParamList, GENERIC_PARAM_LIST);
ast_node!(GenericParam, GENERIC_PARAM);
ast_node!(GenericArgList, GENERIC_ARG_LIST);
ast_node!(GenericArg, GENERIC_ARG);
ast_node!(Visibility, VISIBILITY);

// v0.27 Track A: `@tool(...)` attribute prefix.
ast_node!(ToolAttr, TOOL_ATTR);
ast_node!(ToolAttrArgs, TOOL_ATTR_ARGS);
ast_node!(ToolAttrCapArg, TOOL_ATTR_CAP_ARG);

// Common accessors:
impl File {
    pub fn items(&self) -> impl Iterator<Item = SyntaxNode> + '_ {
        self.0.children()
    }
}

impl Name {
    pub fn text(&self) -> String {
        self.0
            .first_token()
            .map(|t| t.text().to_string())
            .unwrap_or_default()
    }
}

impl Path {
    pub fn segments(&self) -> impl Iterator<Item = PathSegment> + '_ {
        self.0.children().filter_map(PathSegment::cast)
    }
    pub fn text(&self) -> String {
        self.0.text().to_string()
    }
}

impl FnDecl {
    pub fn name(&self) -> Option<Name> {
        self.0.children().find_map(Name::cast)
    }
    pub fn param_list(&self) -> Option<FnParamList> {
        self.0.children().find_map(FnParamList::cast)
    }
    pub fn ret_type(&self) -> Option<RetType> {
        self.0.children().find_map(RetType::cast)
    }
    pub fn effect_clause(&self) -> Option<EffectClause> {
        self.0.children().find_map(EffectClause::cast)
    }
    pub fn body(&self) -> Option<Block> {
        self.0.children().find_map(Block::cast)
    }
    pub fn is_pub(&self) -> bool {
        self.0
            .children()
            .any(|c| c.kind() == SyntaxKind::VISIBILITY)
    }
    pub fn is_unsafe(&self) -> bool {
        self.0
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .any(|t| t.kind() == SyntaxKind::UNSAFE_KW)
    }
}

impl AgentDecl {
    pub fn name(&self) -> Option<Name> {
        self.0.children().find_map(Name::cast)
    }
    pub fn ctor_params(&self) -> Option<AgentCtorParams> {
        self.0.children().find_map(AgentCtorParams::cast)
    }
    pub fn protocols(&self) -> Option<AgentProtocolList> {
        self.0.children().find_map(AgentProtocolList::cast)
    }
    pub fn handlers(&self) -> impl Iterator<Item = OnHandler> + '_ {
        self.0.descendants().filter_map(OnHandler::cast)
    }
    pub fn state_fields(&self) -> impl Iterator<Item = AgentStateDecl> + '_ {
        self.0.descendants().filter_map(AgentStateDecl::cast)
    }
}
