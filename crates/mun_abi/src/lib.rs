//! The Mun ABI
//!
//! The Mun ABI defines the binary format used to communicate between the Mun Compiler and Mun
//! Runtime.
#![warn(missing_docs)]

// C bindings can be manually generated by running `cargo gen-abi`.
mod assembly_info;
mod dispatch_table;
mod function_info;
mod module_info;
mod static_type_map;
mod struct_info;
mod type_info;
mod type_lut;

#[cfg(test)]
mod test_utils;

use std::ffi::{CStr, CString};

pub use assembly_info::AssemblyInfo;
pub use dispatch_table::DispatchTable;
pub use function_info::{
    FunctionDefinition, FunctionDefinitionStorage, FunctionPrototype, FunctionSignature,
    IntoFunctionDefinition,
};
pub use module_info::ModuleInfo;
use once_cell::sync::OnceCell;
pub use struct_info::{StructInfo, StructMemoryKind};
pub use type_info::{HasStaticTypeInfo, TypeInfo, TypeInfoData};
pub use type_lut::{TypeId, TypeLut};

/// The Mun ABI prelude
///
/// The *prelude* contains imports that are used almost every time.
pub mod prelude {
    pub use crate::{HasStaticTypeInfo, IntoFunctionDefinition, StructMemoryKind};
}

/// Defines the current ABI version
#[allow(clippy::zero_prefixed_literal)]
pub const ABI_VERSION: u32 = 00_03_00;
/// Defines the name for the `get_info` function
pub const GET_INFO_FN_NAME: &str = "get_info";
/// Defines the name for the `get_version` function
pub const GET_VERSION_FN_NAME: &str = "get_version";
/// Defines the name for the `set_allocator_handle` function
pub const SET_ALLOCATOR_HANDLE_FN_NAME: &str = "set_allocator_handle";

/// Represents a globally unique identifier (GUID).
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct Guid(pub [u8; 16]);

impl From<&[u8]> for Guid {
    fn from(bytes: &[u8]) -> Self {
        Guid(md5::compute(&bytes).0)
    }
}

impl Guid {
    pub fn empty() -> Guid {
        // TODO: Once `const_fn` lands, replace this with a const md5 hash
        static GUID: OnceCell<Guid> = OnceCell::new();
        *GUID.get_or_init(|| Guid::from("()".as_bytes()))
    }
}

/// Represents the privacy level of modules, functions, or variables.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Privacy {
    /// Publicly (and privately) accessible
    Public = 0,
    /// Privately accessible
    Private = 1,
}

// TODO: Fix leakage of pointer types in struct fields due to integration tests and test utils
