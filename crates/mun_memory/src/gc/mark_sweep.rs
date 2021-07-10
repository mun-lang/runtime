use crate::{cast, gc::{Event, GcPtr, GcRuntime, Observer, RawGcPtr, Stats, TypeTrace}, mapping::{self, FieldMapping, MemoryMapper}, TypeDesc, TypeMemory, TypeComposition, ArrayType};
use mapping::{Conversion, Mapping};
use parking_lot::RwLock;
use std::alloc::{Layout, LayoutError};
use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
    ops::Deref,
    pin::Pin,
    ptr::NonNull,
};

pub trait MarkSweepType: Clone + TypeMemory + TypeTrace + TypeComposition {}
impl<T: Clone + TypeMemory + TypeTrace + TypeComposition> MarkSweepType for T {}

/// Implements a simple mark-sweep type garbage collector.
#[derive(Debug)]
pub struct MarkSweep<T, O>
where
    T: MarkSweepType,
    O: Observer<Event = Event>,
{
    objects: RwLock<HashMap<GcPtr, Pin<Box<ObjectInfo<T>>>>>,
    observer: O,
    stats: RwLock<Stats>,
}

impl<T, O> Default for MarkSweep<T, O>
where
    T: MarkSweepType,
    O: Observer<Event = Event> + Default,
{
    fn default() -> Self {
        MarkSweep {
            objects: RwLock::new(HashMap::new()),
            observer: O::default(),
            stats: RwLock::new(Stats::default()),
        }
    }
}

impl<T, O> MarkSweep<T, O>
where
    T: MarkSweepType,
    O: Observer<Event = Event>,
{
    /// Creates a `MarkSweep` memory collector with the specified `Observer`.
    pub fn with_observer(observer: O) -> Self {
        Self {
            objects: RwLock::new(HashMap::new()),
            observer,
            stats: RwLock::new(Stats::default()),
        }
    }

    /// Logs an allocation
    fn log_alloc(&self, handle: GcPtr, ty: T) {
        {
            let mut stats = self.stats.write();
            stats.allocated_memory += ty.layout().size();
        }

        self.observer.event(Event::Allocation(handle));
    }

    /// Returns the observer
    pub fn observer(&self) -> &O {
        &self.observer
    }
}

/// Allocates memory for an object.
fn alloc_obj<T: MarkSweepType>(ty: T) -> Pin<Box<ObjectInfo<T>>> {
    let ptr = unsafe { std::alloc::alloc(ty.layout()) };
    Box::pin(ObjectInfo {
        ptr,
        length: 1,
        capacity: 1,
        ty,
        roots: 0,
        color: Color::White,
    })
}

/// An error that might occur when requesting memory layout of a type
#[derive(Debug)]
pub enum MemoryLayoutError {
    /// An error that is returned when the memory requested is to large to deal with.
    OutOfBounds,

    /// An error that is returned by constructing a Layout
    LayoutError(LayoutError),
}

impl From<LayoutError> for MemoryLayoutError {
    fn from(err: LayoutError) -> Self {
        MemoryLayoutError::LayoutError(err)
    }
}

/// Creates a layout describing the record for `n` instances of `layout`, with a suitable amount of
/// padding between each to ensure that each instance is given its requested size an alignment.
///
/// Implementation taken from `Layout::repeat` (which is currently unstable)
fn repeat_layout(layout: Layout, n: usize) -> Result<Layout, MemoryLayoutError> {
    let len_rounded_up = layout.size().wrapping_add(layout.align()).wrapping_sub(1)
        & !layout.align().wrapping_sub(1);
    let padded_size = layout.size() + len_rounded_up.wrapping_sub(layout.align());
    let alloc_size = padded_size
        .checked_mul(n)
        .ok_or(MemoryLayoutError::OutOfBounds)?;
    Layout::from_size_align(alloc_size, layout.align()).map_err(Into::into)
}

/// Allocates memory for an array type with `length` elements. `array_ty` must be an array type.
fn alloc_array<T: MarkSweepType>(array_ty: T, length: usize) -> Pin<Box<ObjectInfo<T>>> {
    // Get the element type of the array
    let element_ty = array_ty
        .as_array()
        .expect("array type doesnt have an element type")
        .element_type();

    // Determine the memory layout of the array elements
    let layout = if element_ty.is_stack_allocated() {
        repeat_layout(element_ty.layout(), length)
    } else {
        Layout::array::<GcPtr>(length).map_err(Into::into)
    }
    .unwrap_or_else(|e| {
        panic!(
            "invalid memory layout when allocating an array of {} elements: {:?}",
            length, e
        )
    });

    // Allocate memory for the array elements
    let ptr = unsafe { std::alloc::alloc(layout) };

    Box::pin(ObjectInfo {
        ptr,
        length,
        capacity: length,
        ty: array_ty,
        roots: 0,
        color: Color::White,
    })
}

impl<T, O> GcRuntime<T> for MarkSweep<T, O>
where
    T: MarkSweepType,
    O: Observer<Event = Event>,
{
    fn alloc(&self, ty: T) -> GcPtr {
        let object = alloc_obj(ty.clone());

        // We want to return a pointer to the `ObjectInfo`, to be used as handle.
        let handle = (object.as_ref().deref() as *const _ as RawGcPtr).into();

        {
            let mut objects = self.objects.write();
            objects.insert(handle, object);
        }

        self.log_alloc(handle, ty);
        handle
    }

    fn alloc_array(&self, ty: T, n: usize) -> GcPtr {
        let object = alloc_array(ty.clone(), n);

        // We want to return a pointer to the `ObjectInfo`, to be used as handle.
        let handle = (object.as_ref().deref() as *const _ as RawGcPtr).into();

        {
            let mut objects = self.objects.write();
            objects.insert(handle, object);
        }

        self.log_alloc(handle, ty);
        handle
    }

    fn ptr_type(&self, handle: GcPtr) -> T {
        let _lock = self.objects.read();

        // Convert the handle to our internal representation
        let object_info: *const ObjectInfo<T> = handle.into();

        // Return the type of the object
        unsafe { (*object_info).ty.clone() }
    }

    fn root(&self, handle: GcPtr) {
        let _lock = self.objects.write();

        // Convert the handle to our internal representation
        let object_info: *mut ObjectInfo<T> = handle.into();

        unsafe { (*object_info).roots += 1 };
    }

    fn unroot(&self, handle: GcPtr) {
        let _lock = self.objects.write();

        // Convert the handle to our internal representation
        let object_info: *mut ObjectInfo<T> = handle.into();

        unsafe { (*object_info).roots -= 1 };
    }

    fn stats(&self) -> Stats {
        self.stats.read().clone()
    }
}

impl<T, O> MarkSweep<T, O>
where
    T: MarkSweepType,
    O: Observer<Event = Event>,
{
    /// Collects all memory that is no longer referenced by rooted objects. Returns `true` if memory
    /// was reclaimed, `false` otherwise.
    pub fn collect(&self) -> bool {
        self.observer.event(Event::Start);

        let mut objects = self.objects.write();

        // Get all roots
        let mut roots = objects
            .iter()
            .filter_map(|(_, obj)| {
                if obj.roots > 0 {
                    Some(obj.as_ref().get_ref() as *const _ as *mut ObjectInfo<T>)
                } else {
                    None
                }
            })
            .collect::<VecDeque<_>>();

        // Iterate over all roots
        while let Some(next) = roots.pop_front() {
            let handle = (next as *const _ as RawGcPtr).into();

            // Trace all other objects
            for reference in unsafe { (*next).ty.trace(handle) } {
                let ref_ptr = objects
                    .get_mut(&reference)
                    .expect("found invalid reference");
                if ref_ptr.color == Color::White {
                    let ptr = ref_ptr.as_ref().get_ref() as *const _ as *mut ObjectInfo<T>;
                    unsafe { (*ptr).color = Color::Gray };
                    roots.push_back(ptr);
                }
            }

            // This object has been traced
            unsafe {
                (*next).color = Color::Black;
            }
        }

        // Sweep all non-reachable objects
        let size_before = objects.len();
        objects.retain(|h, obj| {
            if obj.color == Color::Black {
                unsafe {
                    obj.as_mut().get_unchecked_mut().color = Color::White;
                }
                true
            } else {
                unsafe { std::alloc::dealloc(obj.ptr, obj.value_layout()) };
                self.observer.event(Event::Deallocation(*h));
                {
                    let mut stats = self.stats.write();
                    stats.allocated_memory -= obj.ty.layout().size();
                }
                false
            }
        });
        let size_after = objects.len();

        self.observer.event(Event::End);

        size_before != size_after
    }
}

impl<T, O> MemoryMapper<T> for MarkSweep<T, O>
where
    T: TypeDesc + MarkSweepType + Eq + Hash,
    O: Observer<Event = Event>,
{
    fn map_memory(&self, mapping: Mapping<T, T>) -> Vec<GcPtr> {
        let mut objects = self.objects.write();

        // Determine which types are still allocated with deleted types
        let deleted = objects
            .iter()
            .filter_map(|(ptr, object_info)| {
                if mapping.deletions.contains(&object_info.ty) {
                    Some(*ptr)
                } else {
                    None
                }
            })
            .collect();

        // Update type pointers of types that didn't change
        for (old_ty, new_ty) in mapping.identical {
            for object_info in objects.values_mut() {
                if object_info.ty == old_ty {
                    object_info.set(ObjectInfo {
                        ptr: object_info.ptr,
                        length: object_info.length,
                        capacity: object_info.capacity,
                        roots: object_info.roots,
                        color: object_info.color,
                        ty: new_ty.clone(),
                    });
                }
            }
        }

        let mut new_allocations = Vec::new();

        for (old_ty, conversion) in mapping.conversions.iter() {
            for object_info in objects.values_mut() {
                if object_info.ty == *old_ty {
                    let src = unsafe { NonNull::new_unchecked(object_info.ptr) };
                    let dest = unsafe {
                        NonNull::new_unchecked(std::alloc::alloc_zeroed(conversion.new_ty.layout()))
                    };

                    map_fields(
                        self,
                        &mut new_allocations,
                        &mapping.conversions,
                        &conversion.field_mapping,
                        src,
                        dest,
                    );

                    unsafe { std::alloc::dealloc(src.as_ptr(), old_ty.layout()) };

                    object_info.set(ObjectInfo {
                        ptr: dest.as_ptr(),
                        length: object_info.length,
                        capacity: object_info.capacity,
                        roots: object_info.roots,
                        color: object_info.color,
                        ty: conversion.new_ty.clone(),
                    });
                }
            }
        }

        // Retroactively store newly allocated objects
        // This cannot be done while mapping because we hold a mutable reference to objects
        for object in new_allocations {
            let ty = object.ty.clone();
            // We want to return a pointer to the `ObjectInfo`, to
            // be used as handle.
            let handle = (object.as_ref().deref() as *const _ as RawGcPtr).into();
            objects.insert(handle, object);

            self.log_alloc(handle, ty);
        }

        return deleted;

        fn map_fields<T, O>(
            gc: &MarkSweep<T, O>,
            new_allocations: &mut Vec<Pin<Box<ObjectInfo<T>>>>,
            conversions: &HashMap<T, Conversion<T>>,
            mapping: &[FieldMapping<T>],
            src: NonNull<u8>,
            dest: NonNull<u8>,
        ) where
            T: TypeDesc + TypeComposition + MarkSweepType + Eq + Hash,
            O: Observer<Event = Event>,
        {
            for FieldMapping {
                new_ty,
                new_offset,
                action,
            } in mapping.iter()
            {
                let field_dest = {
                    let mut dest = dest.as_ptr() as usize;
                    dest += new_offset;
                    dest as *mut u8
                };

                match action {
                    mapping::Action::Cast { old_offset, old_ty } => {
                        let field_src = {
                            let mut src = src.as_ptr() as usize;
                            src += old_offset;
                            src as *mut u8
                        };

                        if old_ty.is_struct() {
                            debug_assert!(new_ty.is_struct());

                            // When the name is the same, we are dealing with the same struct,
                            // but different internals
                            let is_same_struct = old_ty.name() == new_ty.name();

                            // If the same struct changed, there must also be a conversion
                            let conversion = conversions.get(old_ty);

                            if old_ty.is_stack_allocated() {
                                if new_ty.is_stack_allocated() {
                                    // struct(value) -> struct(value)
                                    if is_same_struct {
                                        // Map in-memory struct to in-memory struct
                                        map_fields(
                                            gc,
                                            new_allocations,
                                            conversions,
                                            &conversion.as_ref().unwrap().field_mapping,
                                            unsafe { NonNull::new_unchecked(field_src) },
                                            unsafe { NonNull::new_unchecked(field_dest) },
                                        );
                                    } else {
                                        // Use previously zero-initialized memory
                                    }
                                } else {
                                    // struct(value) -> struct(gc)
                                    let object = alloc_obj(new_ty.clone());

                                    // We want to return a pointer to the `ObjectInfo`, to be used as handle.
                                    let handle =
                                        (object.as_ref().deref() as *const _ as RawGcPtr).into();

                                    if is_same_struct {
                                        // Map in-memory struct to heap-allocated struct
                                        map_fields(
                                            gc,
                                            new_allocations,
                                            conversions,
                                            &conversion.as_ref().unwrap().field_mapping,
                                            unsafe { NonNull::new_unchecked(field_src) },
                                            unsafe { NonNull::new_unchecked(object.ptr) },
                                        );
                                    } else {
                                        // Zero initialize heap-allocated object
                                        unsafe {
                                            std::ptr::write_bytes(
                                                (*object).ptr,
                                                0,
                                                new_ty.layout().size(),
                                            )
                                        };
                                    }

                                    // Write handle to field
                                    let field_handle = field_dest.cast::<GcPtr>();
                                    unsafe { *field_handle = handle };

                                    new_allocations.push(object);
                                }
                            } else if !new_ty.is_stack_allocated() {
                                // struct(gc) -> struct(gc)
                                let field_src = field_src.cast::<GcPtr>();
                                let field_dest = field_dest.cast::<GcPtr>();

                                if is_same_struct {
                                    // Only copy the `GcPtr`. Memory will already be mapped.
                                    unsafe {
                                        *field_dest = *field_src;
                                    }
                                } else {
                                    let object = alloc_obj(new_ty.clone());

                                    // We want to return a pointer to the `ObjectInfo`, to
                                    // be used as handle.
                                    let handle =
                                        (object.as_ref().deref() as *const _ as RawGcPtr).into();

                                    // Zero-initialize heap-allocated object
                                    unsafe {
                                        std::ptr::write_bytes(object.ptr, 0, new_ty.layout().size())
                                    };

                                    // Write handle to field
                                    unsafe {
                                        *field_dest = handle;
                                    }

                                    new_allocations.push(object);
                                }
                            } else {
                                // struct(gc) -> struct(value)
                                let field_handle = unsafe { *field_src.cast::<GcPtr>() };

                                // Convert the handle to our internal representation
                                // Safety: we already hold a write lock on `objects`, so
                                // this is legal.
                                let obj: *mut ObjectInfo<T> = field_handle.into();
                                let obj = unsafe { &*obj };

                                if is_same_struct {
                                    if obj.ty == *old_ty {
                                        // The object still needs to be mapped
                                        // Map heap-allocated struct to in-memory struct
                                        map_fields(
                                            gc,
                                            new_allocations,
                                            conversions,
                                            &conversion.as_ref().unwrap().field_mapping,
                                            unsafe { NonNull::new_unchecked(obj.ptr) },
                                            unsafe { NonNull::new_unchecked(field_dest) },
                                        );
                                    } else {
                                        // The object was already mapped
                                        debug_assert!(obj.ty == *new_ty);

                                        // Copy from heap-allocated struct to in-memory struct
                                        unsafe {
                                            std::ptr::copy_nonoverlapping(
                                                obj.ptr,
                                                field_dest,
                                                obj.ty.layout().size(),
                                            )
                                        };
                                    }
                                } else {
                                    // Use previously zero-initialized memory
                                }
                            }
                        } else if !cast::try_cast_from_to(
                            *old_ty.guid(),
                            *new_ty.guid(),
                            unsafe { NonNull::new_unchecked(field_src) },
                            unsafe { NonNull::new_unchecked(field_dest) },
                        ) {
                            // Failed to cast. Use the previously zero-initialized value instead
                        }
                    }
                    mapping::Action::Copy { old_offset } => {
                        let field_src = {
                            let mut src = src.as_ptr() as usize;
                            src += old_offset;
                            src as *mut u8
                        };

                        unsafe {
                            std::ptr::copy_nonoverlapping(
                                field_src,
                                field_dest,
                                new_ty.layout().size(),
                            )
                        };
                    }
                    mapping::Action::Insert => {
                        if !new_ty.is_stack_allocated() {
                            let object = alloc_obj(new_ty.clone());

                            // We want to return a pointer to the `ObjectInfo`, to be used as
                            // handle.
                            let handle = (object.as_ref().deref() as *const _ as RawGcPtr).into();

                            // Zero-initialize heap-allocated object
                            unsafe { std::ptr::write_bytes(object.ptr, 0, new_ty.layout().size()) };

                            // Write handle to field
                            let field_dest = field_dest.cast::<GcPtr>();
                            unsafe {
                                *field_dest = handle;
                            }

                            new_allocations.push(object);
                        } else {
                            // Use the previously zero-initialized value
                        }
                    }
                }
            }
        }
    }
}

/// Coloring used in the Mark Sweep phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Color {
    /// A white object has not been seen yet by the mark phase
    White,

    /// A gray object has been seen by the mark phase but has not yet been visited
    Gray,

    /// A black object has been visited by the mark phase
    Black,
}

/// An indirection table that stores the address to the actual memory, the type of the object and
/// meta information.
#[derive(Debug)]
#[repr(C)]
struct ObjectInfo<T: MarkSweepType> {
    pub ptr: *mut u8,
    pub length: usize,
    pub capacity: usize,
    pub roots: u32,
    pub color: Color,
    pub ty: T,
}

impl<T: MarkSweepType> ObjectInfo<T> {
    /// Returns the memory layout of the value stored with this object
    pub fn value_layout(&self) -> Layout {
        if let Some(array_type) = self.ty.as_array() {
            let element_type = array_type.element_type();
            if element_type.is_stack_allocated() {
                repeat_layout(element_type.layout(), self.capacity)
            } else {
                Layout::array::<GcPtr>(self.capacity).map_err(Into::into)
            }
            .expect("must have a valid layout since it has already been allocated")
        } else {
            self.ty.layout()
        }
    }
}

/// An `ObjectInfo` is thread-safe.
unsafe impl<T: MarkSweepType> Send for ObjectInfo<T> {}
unsafe impl<T: MarkSweepType> Sync for ObjectInfo<T> {}

impl<T: MarkSweepType> From<GcPtr> for *const ObjectInfo<T> {
    fn from(ptr: GcPtr) -> Self {
        ptr.as_ptr() as Self
    }
}

impl<T: MarkSweepType> From<GcPtr> for *mut ObjectInfo<T> {
    fn from(ptr: GcPtr) -> Self {
        ptr.as_ptr() as Self
    }
}

impl<T: MarkSweepType> From<*const ObjectInfo<T>> for GcPtr {
    fn from(info: *const ObjectInfo<T>) -> Self {
        (info as RawGcPtr).into()
    }
}

impl<T: MarkSweepType> From<*mut ObjectInfo<T>> for GcPtr {
    fn from(info: *mut ObjectInfo<T>) -> Self {
        (info as RawGcPtr).into()
    }
}
