//! The Mun ABI
//!
//! The Mun ABI defines the binary format used to communicate between the Mun Compiler and Mun
//! Runtime.
#![warn(missing_docs)]

use std::ffi::CStr;
use std::fmt;

pub use assembly_info::AssemblyInfo;
pub use dispatch_table::DispatchTable;
pub use function_info::{FunctionDefinition, FunctionPrototype, FunctionSignature};
pub use module_info::ModuleInfo;
pub use primitive::PrimitiveType;
pub use struct_info::{StructDefinition, StructMemoryKind};
pub use type_id::HasStaticTypeId;
pub use type_id::{ArrayTypeId, PointerTypeId, TypeId};
pub use type_info::{HasStaticTypeName, TypeDefinition, TypeDefinitionData};
pub use type_lut::TypeLut;

// C bindings can be manually generated by running `cargo gen-abi`.
mod assembly_info;
mod dispatch_table;
mod function_info;
mod module_info;
mod primitive;
pub mod static_type_map;
mod struct_info;
mod type_id;
mod type_info;
mod type_lut;

#[cfg(test)]
mod test_utils;

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

impl Guid {
    /// Create a GUID from a string by computing its hash.
    pub const fn from_str(str: &str) -> Guid {
        Guid(extendhash::md5::compute_hash(str.as_bytes()))
    }

    /// Create a GUID from a string by computing its hash.
    pub fn from_cstr(str: &CStr) -> Guid {
        Guid(extendhash::md5::compute_hash(str.to_bytes()))
    }
}

impl fmt::Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hyphenated = format_hyphenated(&self.0);

        // SAFETY: The encoded buffer is ASCII encoded
        let hyphenated = unsafe { std::str::from_utf8_unchecked(&hyphenated) };

        return f.write_str(hyphenated);

        #[inline]
        const fn format_hyphenated(src: &[u8; 16]) -> [u8; 36] {
            const LUT: [u8; 16] = [
                b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'a', b'b', b'c', b'd',
                b'e', b'f',
            ];

            let groups = [(0, 8), (9, 13), (14, 18), (19, 23), (24, 36)];
            let mut dst = [0; 36];

            let mut group_idx = 0;
            let mut i = 0;
            while group_idx < 5 {
                let (start, end) = groups[group_idx];
                let mut j = start;
                while j < end {
                    let x = src[i];
                    i += 1;

                    dst[j] = LUT[(x >> 4) as usize];
                    dst[j + 1] = LUT[(x & 0x0f) as usize];
                    j += 2;
                }
                if group_idx < 4 {
                    dst[end] = b'-';
                }
                group_idx += 1;
            }
            dst
        }
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Guid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{}", self))
    }
}

/// Represents the privacy level of modules, functions, or variables.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Privacy {
    /// Publicly (and privately) accessible
    Public = 0,
    /// Privately accessible
    Private = 1,
}

// TODO: Fix leakage of pointer types in struct fields due to integration tests and test utils
