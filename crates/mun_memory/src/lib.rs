pub use type_info::{FieldInfo, HasStaticTypeInfo, StructInfo, TypeInfo, TypeInfoData};

mod cast;
pub mod diff;
pub mod gc;
pub mod mapping;
mod type_info;
pub mod type_table;

pub mod prelude {
    pub use crate::diff::{diff, Diff, FieldDiff, FieldEditKind};
    pub use crate::mapping::{Action, FieldMapping};
    pub use crate::type_info::{StructInfo, TypeInfo, TypeInfoData};
}

/// A trait used to obtain a type's fields.
pub trait TypeFields: Send + Sync {
    /// Returns the type's fields.
    fn fields(&self) -> &[FieldInfo];
}
