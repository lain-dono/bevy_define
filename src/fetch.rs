use crate::{
    DefComponent, DefineRegister,
    any_def::{AnyRead, AnyWrite},
};
use bevy_ecs::{
    change_detection::{MaybeLocation, Tick},
    component::{ComponentId, StorageType},
    entity::Entity,
    query::{DebugCheckedUnwrap as _, FilteredAccess},
    storage::{ComponentSparseSet, SparseSets, Table, TableRow},
    world::{Mut, Ref, World, unsafe_world_cell::UnsafeWorldCell},
};
use bevy_ptr::{Ptr, ThinSlicePtr, UnsafeCellDeref as _};
use std::cell::UnsafeCell;
use std::{marker::PhantomData, panic::Location};

/// The [`WorldQuery::Fetch`] type for `& T`.
pub struct DefReadFetch<'w, T: DefComponent> {
    pub(crate) components: StorageSwitch<
        T,
        // T::STORAGE_TYPE = StorageType::Table
        Option<ThinSlicePtr<'w, UnsafeCell<T>>>,
        // T::STORAGE_TYPE = StorageType::SparseSet
        Option<&'w ComponentSparseSet>,
    >,
}

impl<T: DefComponent> Copy for DefReadFetch<'_, T> {}
impl<T: DefComponent> Clone for DefReadFetch<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

pub struct DefTickFetch<'w, T: DefComponent> {
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

impl<T: DefComponent> Copy for DefTickFetch<'_, T> {}
impl<T: DefComponent> Clone for DefTickFetch<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'w, T: DefComponent> DefTickFetch<'w, T> {
    pub(crate) fn init(
        world: UnsafeWorldCell<'w>,
        component_id: ComponentId,
        last_run: Tick,
        this_run: Tick,
    ) -> Self {
        Self {
            components: StorageSwitch::new(
                || None,
                || {
                    // SAFETY: The underlying type associated with `component_id` is `T`,
                    // which we are allowed to access since we registered it in `update_component_access`.
                    // Note that we do not actually access any components in this function, we just get a shared
                    // reference to the sparse set, which is used to access the components in `Self::fetch`.
                    unsafe { world.storages().sparse_sets.get(component_id) }
                },
            ),
            last_run,
            this_run,
        }
    }

    /// SAFETY: set_table must be called when T::STORAGE_TYPE = StorageType::Table
    pub(crate) unsafe fn set_table(&mut self, component_id: ComponentId, table: &'w Table) {
        unsafe {
            let column = table.get_column(component_id).debug_checked_unwrap();
            let table_data = Some((
                column.get_data_slice(table.entity_count() as usize).into(),
                column
                    .get_added_ticks_slice(table.entity_count() as usize)
                    .into(),
                column
                    .get_changed_ticks_slice(table.entity_count() as usize)
                    .into(),
                column
                    .get_changed_by_slice(table.entity_count() as usize)
                    .map(Into::into),
            ));
            self.components.set_table(table_data);
        }
    }

    pub(crate) fn fetch_ref(&mut self, entity: Entity, row: TableRow) -> Ref<'w, T> {
        let (value, added, changed, caller) = unsafe { self.fetch(entity, row) };

        Ref::new(
            unsafe { value.deref() },
            unsafe { added.deref() },
            unsafe { changed.deref() },
            self.last_run,
            self.this_run,
            unsafe { caller.map(|caller| caller.deref()) },
        )
    }

    pub(crate) fn fetch_mut(&mut self, entity: Entity, row: TableRow) -> Mut<'w, T> {
        let (value, added, changed, caller) = unsafe { self.fetch(entity, row) };

        Mut::new(
            unsafe { value.assert_unique().deref_mut() },
            // SAFETY: Caller ensures there is no mutable access to the cell.
            unsafe { added.deref_mut() },
            // SAFETY: Caller ensures there is no mutable access to the cell.
            unsafe { changed.deref_mut() },
            self.last_run,
            self.this_run,
            // SAFETY: Caller ensures there is no mutable access to the cell.
            unsafe { caller.map(|caller| caller.deref_mut()) },
        )
    }

    pub(crate) unsafe fn fetch(
        &mut self,
        entity: Entity,
        row: TableRow,
    ) -> (
        Ptr<'w>,
        &'w UnsafeCell<Tick>,
        &'w UnsafeCell<Tick>,
        MaybeLocation<&'w UnsafeCell<&'static Location<'static>>>,
    ) {
        let (value, added, changed, caller) = self.components.extract(
            |table| {
                // SAFETY: set_table was previously called
                let (table_components, added_ticks, changed_ticks, callers) =
                    unsafe { table.debug_checked_unwrap() };

                // SAFETY: The caller ensures `table_row` is in range.
                let component = unsafe { table_components.get_unchecked(row.index()) };
                // SAFETY: The caller ensures `table_row` is in range.
                let added = unsafe { added_ticks.get_unchecked(row.index()) };
                // SAFETY: The caller ensures `table_row` is in range.
                let changed = unsafe { changed_ticks.get_unchecked(row.index()) };
                // SAFETY: The caller ensures `table_row` is in range.
                let caller = callers.map(|callers| unsafe { callers.get_unchecked(row.index()) });

                unsafe { (Ptr::from(&*(component.get())), added, changed, caller) }
            },
            |sparse_set| {
                // SAFETY: The caller ensures `entity` is in range and has the component.
                let (component, ticks) = unsafe {
                    sparse_set
                        .debug_checked_unwrap()
                        .get_with_ticks(entity)
                        .debug_checked_unwrap()
                };

                (component, ticks.added, ticks.changed, ticks.changed_by)
            },
        );

        (value, added, changed, caller)
    }
}

pub struct AnyQueryState<T: DefComponent> {
    components: Box<[ComponentId]>,
    marker: PhantomData<T>,
}

impl<T: DefComponent> AnyQueryState<T> {
    pub(crate) fn init(world: &mut World) -> Self {
        Self {
            components: world
                .get_resource_or_init::<DefineRegister>()
                .types::<T>()
                .clone()
                .into_boxed_slice(),
            marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn matches(&self, f: &impl Fn(ComponentId) -> bool) -> bool {
        self.components.iter().copied().any(f)
    }

    #[inline]
    pub(crate) fn update<const WRITE: bool>(&self, access: &mut FilteredAccess) {
        let mut new_access = access.clone();
        let mut components = self.components.iter();

        if let Some(&component) = components.next() {
            assert!(
                !access.access().has_write(component),
                "{} conflicts with a previous access in this query",
                core::any::type_name::<T>(),
            );
            let mut intermediate = access.clone();
            if WRITE {
                intermediate.add_write(component);
            } else {
                intermediate.add_read(component);
            }
            new_access.append_or(&intermediate);
            new_access.extend_access(&intermediate);
        }

        for &component in components {
            assert!(
                !access.access().has_write(component),
                "{} conflicts with a previous access in this query",
                core::any::type_name::<T>(),
            );
            new_access.and_with(component);
            let access = new_access.access_mut();
            if WRITE {
                access.add_write(component);
            } else {
                access.add_read(component);
            }
        }
        *access = new_access;
    }
}

pub struct AnyFetch<'w, T: DefComponent> {
    registry: &'w DefineRegister,
    table: Option<&'w Table>,
    sets: &'w SparseSets,
    last_run: Tick,
    this_run: Tick,
    marker: PhantomData<T>,
}

impl<'w, T: DefComponent> Copy for AnyFetch<'w, T> {}
impl<'w, T: DefComponent> Clone for AnyFetch<'w, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'w, T: DefComponent> AnyFetch<'w, T> {
    pub(crate) unsafe fn init(world: UnsafeWorldCell<'w>, last_run: Tick, this_run: Tick) -> Self {
        Self {
            registry: world.get_resource().unwrap(),
            table: None,
            sets: &world.storages().sparse_sets,
            last_run,
            this_run,
            marker: PhantomData,
        }
    }

    pub fn set_table(&mut self, table: &'w Table) {
        self.table = Some(table);
    }

    pub(crate) fn read(&mut self, entity: Entity, row: TableRow) -> AnyRead<'w, T> {
        AnyRead {
            registry: self.registry,
            data: StorageSwitch::new(|| (row, self.table.unwrap()), || (entity, self.sets)),
            last_run: self.last_run,
            this_run: self.this_run,
        }
    }

    pub(crate) fn write(&mut self, entity: Entity, row: TableRow) -> AnyWrite<'w, T> {
        AnyWrite {
            registry: self.registry,
            data: StorageSwitch::new(|| (row, self.table.unwrap()), || (entity, self.sets)),
            last_run: self.last_run,
            this_run: self.this_run,
        }
    }
}

/// A compile-time checked union of two different types that differs based on the
/// [`StorageType`] of a given component.
#[expect(clippy::default_union_representation)]
pub(crate) union StorageSwitch<C: DefComponent, T: Copy, S: Copy> {
    /// The table variant. Requires the component to be a table component.
    pub(crate) table: T,
    /// The sparse set variant. Requires the component to be a sparse set component.
    pub(crate) sparse_set: S,
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

impl<C: DefComponent, T: Copy, S: Copy> Copy for StorageSwitch<C, T, S> {}
impl<C: DefComponent, T: Copy, S: Copy> Clone for StorageSwitch<C, T, S> {
    fn clone(&self) -> Self {
        *self
    }
}
