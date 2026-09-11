use super::{DefComponent, DefineRegister, StorageSwitch, def::Def};
use bevy_ecs::{
    archetype::Archetype,
    change_detection::{ContiguousMut, MaybeLocation, Tick},
    component::{ComponentId, Components, Mutable, StorageType},
    entity::Entity,
    query::{
        ArchetypeQueryData, ContiguousQueryData, DebugCheckedUnwrap as _, EcsAccessLevel,
        EcsAccessType, FilteredAccess, IterQueryData, QueryData, ReleaseStateQueryData,
        SingleEntityQueryData, WorldQuery,
    },
    storage::{ComponentSparseSet, Table, TableRow},
    world::{Mut, World, unsafe_world_cell::UnsafeWorldCell},
};
use bevy_ptr::{ThinSlicePtr, UnsafeCellDeref as _};
use bevy_utils::prelude::DebugName;
use std::{cell::UnsafeCell, iter, marker::PhantomData, panic::Location};

pub struct DefMut<T, const N: usize = 0>(PhantomData<T>);

/// The [`WorldQuery::Fetch`] type for `&mut T`.
pub struct DefWriteFetch<'w, T: DefComponent> {
    components: StorageSwitch<
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
    last_run: Tick,
    this_run: Tick,
}

impl<T: DefComponent> Clone for DefWriteFetch<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: DefComponent> Copy for DefWriteFetch<'_, T> {}

// SAFETY:
// `fetch` accesses a single component mutably.
// This is sound because `update_component_access` adds write access for that component and panic when appropriate.
// `update_component_access` adds a `With` filter for a component.
// This is sound because `matches_component_set` returns whether the set contains that component.
unsafe impl<T: DefComponent<Mutability = Mutable>, const N: usize> WorldQuery for DefMut<T, N> {
    type Fetch<'w> = DefWriteFetch<'w, T>;
    type State = ComponentId;

    fn shrink_fetch<'wlong: 'wshort, 'wshort>(fetch: Self::Fetch<'wlong>) -> Self::Fetch<'wshort> {
        fetch
    }

    #[inline]
    unsafe fn init_fetch<'w>(
        world: UnsafeWorldCell<'w>,
        &component_id: &ComponentId,
        last_run: Tick,
        this_run: Tick,
    ) -> DefWriteFetch<'w, T> {
        DefWriteFetch {
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

    const IS_DENSE: bool = {
        match T::STORAGE_TYPE {
            StorageType::Table => true,
            StorageType::SparseSet => false,
        }
    };

    #[inline]
    unsafe fn set_archetype<'w>(
        fetch: &mut DefWriteFetch<'w, T>,
        component_id: &ComponentId,
        _archetype: &'w Archetype,
        table: &'w Table,
    ) {
        if Self::IS_DENSE {
            // SAFETY: `set_archetype`'s safety rules are a super set of the `set_table`'s ones.
            unsafe {
                Self::set_table(fetch, component_id, table);
            }
        }
    }

    #[inline]
    unsafe fn set_table<'w>(
        fetch: &mut DefWriteFetch<'w, T>,
        &component_id: &ComponentId,
        table: &'w Table,
    ) {
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
        // SAFETY: set_table is only called when T::STORAGE_TYPE = StorageType::Table
        unsafe { fetch.components.set_table(table_data) };
    }

    fn update_component_access(&component_id: &ComponentId, access: &mut FilteredAccess) {
        assert!(
            !access.access().has_read(component_id),
            "&mut {} conflicts with a previous access in this query. Mutable component access must be unique.",
            DebugName::type_name::<T>(),
        );
        access.add_write(component_id);
    }

    fn init_state(world: &mut World) -> ComponentId {
        DefineRegister::<T::Define>::arg_scope::<_, N>(world, |world, key, mut def| {
            def.component::<T>(world, key.clone()).0
        })
    }

    fn get_state(_components: &Components) -> Option<Self::State> {
        None
    }

    fn matches_component_set(
        &state: &ComponentId,
        set_contains_id: &impl Fn(ComponentId) -> bool,
    ) -> bool {
        set_contains_id(state)
    }
}

// SAFETY: access of `&T` is a subset of `&mut T`
unsafe impl<T: DefComponent<Mutability = Mutable>, const N: usize> QueryData for DefMut<T, N> {
    const IS_READ_ONLY: bool = false;
    const IS_ARCHETYPAL: bool = true;
    type ReadOnly = Def<T>;
    type Item<'w, 's> = Mut<'w, T>;

    fn shrink<'wlong: 'wshort, 'wshort, 's>(
        item: Self::Item<'wlong, 's>,
    ) -> Self::Item<'wshort, 's> {
        item
    }

    #[inline(always)]
    unsafe fn fetch<'w, 's>(
        _state: &'s Self::State,
        fetch: &mut Self::Fetch<'w>,
        entity: Entity,
        table_row: TableRow,
    ) -> Option<Self::Item<'w, 's>> {
        Some(fetch.components.extract(
            |table| {
                // SAFETY: set_table was previously called
                let (table_components, added_ticks, changed_ticks, callers) =
                    unsafe { table.debug_checked_unwrap() };

                // SAFETY: The caller ensures `table_row` is in range.
                let component = unsafe { table_components.get_unchecked(table_row.index()) };
                // SAFETY: The caller ensures `table_row` is in range.
                let added = unsafe { added_ticks.get_unchecked(table_row.index()) };
                // SAFETY: The caller ensures `table_row` is in range.
                let changed = unsafe { changed_ticks.get_unchecked(table_row.index()) };
                // SAFETY: The caller ensures `table_row` is in range.
                let caller =
                    callers.map(|callers| unsafe { callers.get_unchecked(table_row.index()) });

                Mut::new(
                    component.deref_mut(),
                    added.deref_mut(),
                    changed.deref_mut(),
                    fetch.this_run,
                    fetch.last_run,
                    caller.map(|caller| caller.deref_mut()),
                )
            },
            |sparse_set| {
                // SAFETY: The caller ensures `entity` is in range and has the component.
                let (component, ticks) = unsafe {
                    sparse_set
                        .debug_checked_unwrap()
                        .get_with_ticks(entity)
                        .debug_checked_unwrap()
                };

                Mut::new(
                    component.assert_unique().deref_mut(),
                    // SAFETY: Caller ensures there is no mutable access to the cell.
                    unsafe { ticks.added.deref_mut() },
                    // SAFETY: Caller ensures there is no mutable access to the cell.
                    unsafe { ticks.changed.deref_mut() },
                    fetch.last_run,
                    fetch.this_run,
                    // SAFETY: Caller ensures there is no mutable access to the cell.
                    unsafe { ticks.changed_by.map(|changed_by| changed_by.deref_mut()) },
                )
            },
        ))
    }

    fn iter_access(state: &Self::State) -> impl Iterator<Item = EcsAccessType<'_>> {
        iter::once(EcsAccessType::Component(EcsAccessLevel::Write(*state)))
    }
}

// SAFETY: access is only on the current entity
unsafe impl<T: DefComponent<Mutability = Mutable>, const N: usize> IterQueryData for DefMut<T, N> {}

// SAFETY: access is only on the current entity
unsafe impl<T: DefComponent<Mutability = Mutable>, const N: usize> SingleEntityQueryData
    for DefMut<T, N>
{
}

impl<T: DefComponent<Mutability = Mutable>, const N: usize> ReleaseStateQueryData for DefMut<T, N> {
    fn release_state<'w>(item: Self::Item<'w, '_>) -> Self::Item<'w, 'static> {
        item
    }
}

impl<T: DefComponent<Mutability = Mutable>, const N: usize> ArchetypeQueryData for DefMut<T, N> {}

impl<T: DefComponent<Mutability = Mutable>, const N: usize> ContiguousQueryData for DefMut<T, N> {
    type Contiguous<'w, 's> = ContiguousMut<'w, T>;

    unsafe fn fetch_contiguous<'w, 's>(
        _state: &'s Self::State,
        fetch: &mut Self::Fetch<'w>,
        entities: &'w [Entity],
    ) -> Self::Contiguous<'w, 's> {
        fetch.components.extract(
            |table| {
                // SAFETY: set_table was previously called
                let (table_components, added_ticks, changed_ticks, callers) =
                    unsafe { table.debug_checked_unwrap() };

                let len = entities.len();

                ContiguousMut::new(
                    // SAFETY: `entities` has the same length as the rows in the set table.
                    unsafe { table_components.as_mut_slice_unchecked(entities.len()) },
                    // SAFETY:
                    // - The caller ensures the permission to access ticks.
                    // - `entities` has the same length as the rows in the set table hence the
                    // ticks.

                    // SAFETY:
                    // - The caller ensures that `len` is the length of the slice.
                    // - The caller ensures we have permission to read the data.
                    unsafe { added_ticks.as_mut_slice_unchecked(len) },
                    // SAFETY: see above.
                    unsafe { changed_ticks.as_mut_slice_unchecked(len) },
                    fetch.this_run,
                    fetch.last_run,
                    // SAFETY: see above.
                    callers.map(|v| unsafe { v.as_mut_slice_unchecked(len) }),
                )
                .expect("valid ContiguousMut")
            },
            |_| {
                #[cfg(debug_assertions)]
                unreachable!();
                // SAFETY: the caller ensures that [`Self::set_table`] was called beforehand.
                #[cfg(not(debug_assertions))]
                core::hint::unreachable_unchecked();
            },
        )
    }
}
