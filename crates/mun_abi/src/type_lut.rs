use crate::Guid;
use std::{ffi, fmt, slice};

/// Represents a unique identifier for types. The runtime can use this to lookup the corresponding [`TypeInfo`].
#[repr(C)]
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TypeId {
    /// The GUID of the type
    pub guid: Guid,
}

impl From<Guid> for TypeId {
    fn from(guid: Guid) -> Self {
        TypeId { guid }
    }
}

impl fmt::Display for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.guid.fmt(f)
    }
}

/// Represents a lookup table for type information. This is used for runtime linking.
///
/// Type IDs and handles are stored separately for cache efficiency.
#[repr(C)]
pub struct TypeLut {
    /// Type IDs
    pub(crate) type_ids: *const TypeId,
    /// Type information handles
    pub(crate) type_handles: *mut *const ffi::c_void,
    /// Number of types
    pub num_entries: u32,
}

impl TypeLut {
    /// Returns an iterator over pairs of type IDs and type handles.
    pub fn iter(&self) -> impl Iterator<Item = (&TypeId, &*const ffi::c_void)> {
        if self.num_entries == 0 {
            (&[]).iter().zip((&[]).iter())
        } else {
            let ptrs =
                unsafe { slice::from_raw_parts_mut(self.type_handles, self.num_entries as usize) };
            let type_ids =
                unsafe { slice::from_raw_parts(self.type_ids, self.num_entries as usize) };

            type_ids.iter().zip(ptrs.iter())
        }
    }

    /// Returns an iterator over pairs of type IDs and mutable type handles.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&TypeId, &mut *const ffi::c_void)> {
        if self.num_entries == 0 {
            (&[]).iter().zip((&mut []).iter_mut())
        } else {
            let ptrs =
                unsafe { slice::from_raw_parts_mut(self.type_handles, self.num_entries as usize) };
            let type_ids =
                unsafe { slice::from_raw_parts(self.type_ids, self.num_entries as usize) };

            type_ids.iter().zip(ptrs.iter_mut())
        }
    }

    /// Returns mutable type handles.
    pub fn type_handles_mut(&mut self) -> &mut [*const ffi::c_void] {
        if self.num_entries == 0 {
            &mut []
        } else {
            unsafe { slice::from_raw_parts_mut(self.type_handles, self.num_entries as usize) }
        }
    }

    /// Returns type IDs.
    pub fn type_ids(&self) -> &[TypeId] {
        if self.num_entries == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(self.type_ids, self.num_entries as usize) }
        }
    }

    /// Returns a type handle, without doing bounds checking.
    ///
    /// This is generally not recommended, use with caution! Calling this method with an
    /// out-of-bounds index is _undefined behavior_ even if the resulting reference is not used.
    /// For a safe alternative see [get_ptr](#method.get_ptr).
    ///
    /// # Safety
    ///
    /// The `idx` is not bounds checked and should therefor be used with care.
    pub unsafe fn get_type_handle_unchecked(&self, idx: u32) -> *const ffi::c_void {
        *self.type_handles.offset(idx as isize)
    }

    /// Returns a type handle at the given index, or `None` if out of bounds.
    pub fn get_type_handle(&self, idx: u32) -> Option<*const ffi::c_void> {
        if idx < self.num_entries {
            Some(unsafe { self.get_type_handle_unchecked(idx) })
        } else {
            None
        }
    }

    /// Returns a mutable reference to a type handle, without doing bounds checking.
    ///
    /// This is generally not recommended, use with caution! Calling this method with an
    /// out-of-bounds index is _undefined behavior_ even if the resulting reference is not used.
    /// For a safe alternative see [get_ptr_mut](#method.get_ptr_mut).
    ///
    /// # Safety
    ///
    /// The `idx` is not bounds checked and should therefor be used with care.
    pub unsafe fn get_type_handle_unchecked_mut(&mut self, idx: u32) -> &mut *const ffi::c_void {
        &mut *self.type_handles.offset(idx as isize)
    }

    /// Returns a mutable reference to a type handle at the given index, or `None` if out of
    /// bounds.
    pub fn get_type_handle_mut(&mut self, idx: u32) -> Option<&mut *const ffi::c_void> {
        if idx < self.num_entries {
            Some(unsafe { self.get_type_handle_unchecked_mut(idx) })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{fake_type_lut, FAKE_TYPE_ID};
    use std::ptr;

    #[test]
    fn test_type_lut_iter_mut_none() {
        let type_ids = &[];
        let type_ptrs = &mut [];
        let mut type_lut = fake_type_lut(type_ids, type_ptrs);

        let iter = type_ids.iter().zip(type_ptrs.iter_mut());
        assert_eq!(type_lut.iter_mut().count(), iter.count());
    }

    #[test]
    fn test_type_lut_iter_mut_some() {
        let type_ids = &[FAKE_TYPE_ID];
        let type_ptrs = &mut [ptr::null()];
        let mut type_lut = fake_type_lut(type_ids, type_ptrs);

        let iter = type_ids.iter().zip(type_ptrs.iter_mut());
        assert_eq!(type_lut.iter_mut().count(), iter.len());

        for (lhs, rhs) in type_lut.iter_mut().zip(iter) {
            assert_eq!(lhs.0, rhs.0);
            assert_eq!(lhs.1, rhs.1);
        }
    }

    #[test]
    fn test_type_lut_ptrs_mut_none() {
        let type_ids = &[];
        let type_ptrs = &mut [];
        let mut type_lut = fake_type_lut(type_ids, type_ptrs);

        assert_eq!(type_lut.type_handles_mut().len(), 0);
    }

    #[test]
    fn test_type_lut_ptrs_mut_some() {
        let type_ids = &[FAKE_TYPE_ID];
        let type_ptrs = &mut [ptr::null()];
        let mut type_lut = fake_type_lut(type_ids, type_ptrs);

        let result = type_lut.type_handles_mut();
        assert_eq!(result.len(), type_ptrs.len());
        for (lhs, rhs) in result.iter().zip(type_ptrs.iter()) {
            assert_eq!(lhs, rhs);
        }
    }

    #[test]
    fn test_type_lut_type_ids_none() {
        let type_ids = &[];
        let type_ptrs = &mut [];
        let type_lut = fake_type_lut(type_ids, type_ptrs);

        assert_eq!(type_lut.type_ids().len(), 0);
    }

    #[test]
    fn test_type_lut_type_ids_some() {
        let type_ids = &[FAKE_TYPE_ID];
        let type_ptrs = &mut [ptr::null()];
        let type_lut = fake_type_lut(type_ids, type_ptrs);

        let result = type_lut.type_ids();
        assert_eq!(result.len(), type_ids.len());
        for (lhs, rhs) in result.iter().zip(type_ids.iter()) {
            assert_eq!(lhs, rhs);
        }
    }

    #[test]
    fn test_type_lut_get_ptr_unchecked() {
        let type_ids = &[FAKE_TYPE_ID];
        let type_ptrs = &mut [ptr::null()];

        let type_lut = fake_type_lut(type_ids, type_ptrs);
        assert_eq!(
            unsafe { type_lut.get_type_handle_unchecked(0) },
            type_ptrs[0]
        );
    }

    #[test]
    fn test_type_lut_get_ptr_none() {
        let prototype = &[FAKE_TYPE_ID];
        let type_ptrs = &mut [ptr::null()];

        let type_lut = fake_type_lut(prototype, type_ptrs);
        assert_eq!(type_lut.get_type_handle(1), None);
    }

    #[test]
    fn test_type_lut_get_ptr_some() {
        let type_ids = &[FAKE_TYPE_ID];
        let type_ptrs = &mut [ptr::null()];

        let type_lut = fake_type_lut(type_ids, type_ptrs);
        assert_eq!(type_lut.get_type_handle(0), Some(type_ptrs[0]));
    }

    #[test]
    fn test_type_lut_get_ptr_unchecked_mut() {
        let type_ids = &[FAKE_TYPE_ID];
        let type_ptrs = &mut [ptr::null()];

        let mut type_lut = fake_type_lut(type_ids, type_ptrs);
        assert_eq!(
            unsafe { type_lut.get_type_handle_unchecked_mut(0) },
            &mut type_ptrs[0]
        );
    }

    #[test]
    fn test_type_lut_get_ptr_mut_none() {
        let type_ids = &[FAKE_TYPE_ID];
        let type_ptrs = &mut [ptr::null()];

        let mut type_lut = fake_type_lut(type_ids, type_ptrs);
        assert_eq!(type_lut.get_type_handle_mut(1), None);
    }

    #[test]
    fn test_type_lut_get_ptr_mut_some() {
        let type_ids = &[FAKE_TYPE_ID];
        let type_ptrs = &mut [ptr::null()];

        let mut type_lut = fake_type_lut(type_ids, type_ptrs);
        assert_eq!(type_lut.get_type_handle_mut(0), Some(&mut type_ptrs[0]));
    }
}
