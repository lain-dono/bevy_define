use super::{DefComponent, DefineRegister, StorageSwitch};
use bevy_ecs::{
    archetype::Archetype,
    change_detection::Tick,
    component::{ComponentId, Components, StorageType},
    entity::Entity,
    query::{
        ArchetypeQueryData, ContiguousQueryData, DebugCheckedUnwrap as _, EcsAccessLevel,
        EcsAccessType, FilteredAccess, IterQueryData, QueryData, ReadOnlyQueryData,
        ReleaseStateQueryData, SingleEntityQueryData, WorldQuery,
    },
    storage::{ComponentSparseSet, Table, TableRow},
    world::{World, unsafe_world_cell::UnsafeWorldCell},
};
use bevy_ptr::{ThinSlicePtr, UnsafeCellDeref as _};
use bevy_utils::prelude::DebugName;
use std::{cell::UnsafeCell, iter, marker::PhantomData};

pub struct Def<T, const N: usize = 0>(PhantomData<T>);

/// The [`WorldQuery::Fetch`] type for `& T`.
pub struct DefReadFetch<'w, T: DefComponent> {
    components: StorageSwitch<
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

// SAFETY:
// `fetch` accesses a single component in a readonly way.
// This is sound because `update_component_access` adds read access for that component and panic when appropriate.
// `update_component_access` adds a `With` filter for a component.
// This is sound because `matches_component_set` returns whether the set contains that component.
unsafe impl<T: DefComponent, const N: usize> WorldQuery for Def<T, N> {
    type Fetch<'w> = DefReadFetch<'w, T>;
    type State = ComponentId;

    fn shrink_fetch<'wlong: 'wshort, 'wshort>(fetch: Self::Fetch<'wlong>) -> Self::Fetch<'wshort> {
        fetch
    }

    #[inline]
    unsafe fn init_fetch<'w>(
        world: UnsafeWorldCell<'w>,
        &component_id: &ComponentId,
        _last_run: Tick,
        _this_run: Tick,
    ) -> DefReadFetch<'w, T> {
        DefReadFetch {
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
        fetch: &mut DefReadFetch<'w, T>,
        component_id: &ComponentId,
        _archetype: &'w Archetype,
        table: &'w Table,
    ) {
        if Self::IS_DENSE {
            // SAFETY: `set_archetype`'s safety rules are a super set of the `set_table`'s ones.
            unsafe { Self::set_table(fetch, component_id, table) };
        }
    }

    #[inline]
    unsafe fn set_table<'w>(
        fetch: &mut DefReadFetch<'w, T>,
        &component_id: &ComponentId,
        table: &'w Table,
    ) {
        let table_data = Some(unsafe {
            table
                .get_data_slice_for(component_id)
                .debug_checked_unwrap()
                .into()
        });
        // SAFETY: set_table is only called when T::STORAGE_TYPE = StorageType::Table
        unsafe { fetch.components.set_table(table_data) };
    }

    fn update_component_access(&component_id: &ComponentId, access: &mut FilteredAccess) {
        assert!(
            !access.access().has_write(component_id),
            "&{} conflicts with a previous access in this query. Shared access cannot coincide with exclusive access.",
            DebugName::type_name::<T>(),
        );
        access.add_read(component_id);
    }

    fn init_state(world: &mut World) -> ComponentId {
        DefineRegister::arg_component::<T, N>(world).0
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

// SAFETY: `Self` is the same as `Self::ReadOnly`
unsafe impl<T: DefComponent, const N: usize> QueryData for Def<T, N> {
    const IS_READ_ONLY: bool = true;
    const IS_ARCHETYPAL: bool = true;
    type ReadOnly = Self;
    type Item<'w, 's> = &'w T;

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
                let table = unsafe { table.debug_checked_unwrap() };
                // SAFETY: Caller ensures `table_row` is in range.
                let item = unsafe { table.get_unchecked(table_row.index()) };
                item.deref()
            },
            |sparse_set| {
                // SAFETY: Caller ensures `entity` is in range.
                let item = unsafe {
                    sparse_set
                        .debug_checked_unwrap()
                        .get(entity)
                        .debug_checked_unwrap()
                };
                item.deref()
            },
        ))
    }

    fn iter_access(state: &Self::State) -> impl Iterator<Item = EcsAccessType<'_>> {
        iter::once(EcsAccessType::Component(EcsAccessLevel::Read(*state)))
    }
}

impl<T: DefComponent, const N: usize> ContiguousQueryData for Def<T, N> {
    type Contiguous<'w, 's> = &'w [T];

    unsafe fn fetch_contiguous<'w, 's>(
        _state: &'s Self::State,
        fetch: &mut Self::Fetch<'w>,
        entities: &'w [Entity],
    ) -> Self::Contiguous<'w, 's> {
        fetch.components.extract(
            |table| {
                // SAFETY: The caller ensures `set_table` was previously called
                let table = unsafe { table.debug_checked_unwrap() };
                // SAFETY:
                // - `table` is `entities.len()` long
                // - `UnsafeCell<T>` has the same layout as `T`
                unsafe { table.cast().as_slice_unchecked(entities.len()) }
            },
            |_| {
                #[cfg(debug_assertions)]
                unreachable!();
                // SAFETY: The caller ensures query is dense
                #[cfg(not(debug_assertions))]
                core::hint::unreachable_unchecked();
            },
        )
    }
}

// SAFETY: access is read only and only on the current entity
unsafe impl<T: DefComponent, const N: usize> IterQueryData for Def<T, N> {}

// SAFETY: access is read only
unsafe impl<T: DefComponent, const N: usize> ReadOnlyQueryData for Def<T, N> {}

// SAFETY: access is only on the current entity
unsafe impl<T: DefComponent, const N: usize> SingleEntityQueryData for Def<T, N> {}

impl<T: DefComponent, const N: usize> ReleaseStateQueryData for Def<T, N> {
    fn release_state<'w>(item: Self::Item<'w, '_>) -> Self::Item<'w, 'static> {
        item
    }
}

impl<T: DefComponent, const N: usize> ArchetypeQueryData for Def<T, N> {}
