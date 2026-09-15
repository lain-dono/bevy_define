use crate::DefComponent;
use bevy_ecs::{
    change_detection::{MaybeLocation, Tick},
    component::StorageType,
    storage::ComponentSparseSet,
};
use bevy_ptr::ThinSlicePtr;
use std::cell::UnsafeCell;
use std::{marker::PhantomData, panic::Location};

/// The [`WorldQuery::Fetch`] type for `& T`.
#[doc(hidden)]
pub struct DefReadFetch<'w, T: DefComponent> {
    pub(crate) components: StorageSwitch<
        T,
        // T::STORAGE_TYPE = StorageType::Table
        Option<ThinSlicePtr<'w, UnsafeCell<T>>>,
        // T::STORAGE_TYPE = StorageType::SparseSet
        Option<&'w ComponentSparseSet>,
    >,
}

impl<T: DefComponent> Clone for DefReadFetch<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: DefComponent> Copy for DefReadFetch<'_, T> {}

#[doc(hidden)]
pub struct DefRefFetch<'w, T: DefComponent> {
    pub(crate) components: StorageSwitch<
        T,
        // T::STORAGE_TYPE = StorageType::Table
        Option<(
            ThinSlicePtr<'w, UnsafeCell<T>>,
            ThinSlicePtr<'w, UnsafeCell<Tick>>,
            ThinSlicePtr<'w, UnsafeCell<Tick>>,
            MaybeLocation<ThinSlicePtr<'w, UnsafeCell<&'static Location<'static>>>>,
        )>,
        // T::STORAGE_TYPE = StorageType::SparseSet
        // Can be `None` when the component has never been inserted
        Option<&'w ComponentSparseSet>,
    >,
    pub(crate) last_run: Tick,
    pub(crate) this_run: Tick,
}

impl<T: DefComponent> Clone for DefRefFetch<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: DefComponent> Copy for DefRefFetch<'_, T> {}

/// The [`WorldQuery::Fetch`] type for `&mut T`.
pub struct DefWriteFetch<'w, T: DefComponent> {
    pub(crate) components: StorageSwitch<
        T,
        // T::STORAGE_TYPE = StorageType::Table
        Option<(
            ThinSlicePtr<'w, UnsafeCell<T>>,
            ThinSlicePtr<'w, UnsafeCell<Tick>>,
            ThinSlicePtr<'w, UnsafeCell<Tick>>,
            MaybeLocation<ThinSlicePtr<'w, UnsafeCell<&'static Location<'static>>>>,
        )>,
        // T::STORAGE_TYPE = StorageType::SparseSet
        // Can be `None` when the component has never been inserted
        Option<&'w ComponentSparseSet>,
    >,
    pub(crate) last_run: Tick,
    pub(crate) this_run: Tick,
}

impl<T: DefComponent> Clone for DefWriteFetch<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: DefComponent> Copy for DefWriteFetch<'_, T> {}

/// A compile-time checked union of two different types that differs based on the
/// [`StorageType`] of a given component.
#[expect(clippy::default_union_representation)]
pub(crate) union StorageSwitch<C: DefComponent, T: Copy, S: Copy> {
    /// The table variant. Requires the component to be a table component.
    table: T,
    /// The sparse set variant. Requires the component to be a sparse set component.
    sparse_set: S,
    marker: PhantomData<C>,
}

impl<C: DefComponent, T: Copy, S: Copy> StorageSwitch<C, T, S> {
    /// Creates a new [`StorageSwitch`] using the given closures to initialize
    /// the variant corresponding to the component's [`StorageType`].
    pub fn new(table: impl FnOnce() -> T, sparse_set: impl FnOnce() -> S) -> Self {
        match C::STORAGE_TYPE {
            StorageType::Table => Self { table: table() },
            StorageType::SparseSet => Self {
                sparse_set: sparse_set(),
            },
        }
    }

    /// Creates a new [`StorageSwitch`] using a table variant.
    ///
    /// # Panics
    ///
    /// This will panic on debug builds if `C` is not a table component.
    ///
    /// # Safety
    ///
    /// `C` must be a table component.
    #[inline]
    pub unsafe fn set_table(&mut self, table: T) {
        match C::STORAGE_TYPE {
            StorageType::Table => self.table = table,
            #[cfg(debug_assertions)]
            StorageType::SparseSet => unreachable!(),
            #[cfg(not(debug_assertions))]
            StorageType::SparseSet => core::hint::unreachable_unchecked(),
        }
    }

    /// Fetches the internal value from the variant that corresponds to the
    /// component's [`StorageType`].
    pub fn extract<R>(&self, table: impl FnOnce(T) -> R, sparse_set: impl FnOnce(S) -> R) -> R {
        match C::STORAGE_TYPE {
            // SAFETY: C::STORAGE_TYPE == StorageType::Table
            StorageType::Table => table(unsafe { self.table }),
            // SAFETY: C::STORAGE_TYPE == StorageType::SparseSet
            StorageType::SparseSet => sparse_set(unsafe { self.sparse_set }),
        }
    }
}

impl<C: DefComponent, T: Copy, S: Copy> Clone for StorageSwitch<C, T, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C: DefComponent, T: Copy, S: Copy> Copy for StorageSwitch<C, T, S> {}
