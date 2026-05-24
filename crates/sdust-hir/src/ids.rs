use la_arena::Idx;

pub type ItemId = Idx<crate::nodes::Item>;
pub type FnId = Idx<crate::nodes::HirFn>;
pub type StructId = Idx<crate::nodes::HirStruct>;
pub type EnumId = Idx<crate::nodes::HirEnum>;
pub type TypeAliasId = Idx<crate::nodes::HirTypeAlias>;
pub type AgentId = Idx<crate::nodes::HirAgent>;
pub type ProtocolId = Idx<crate::nodes::HirProtocol>;
pub type SupervisorId = Idx<crate::nodes::HirSupervisor>;
pub type ExprId = Idx<crate::nodes::HirExpr>;
pub type PatId = Idx<crate::nodes::HirPat>;
pub type TypeId = Idx<crate::nodes::HirType>;
pub type BlockId = Idx<crate::nodes::HirBlock>;
pub type LocalId = Idx<crate::nodes::HirLocal>;
