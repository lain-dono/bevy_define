use super::{DefComponent, DefineRegister};
use bevy_ecs::{
    archetype::Archetype,
    change_detection::Tick,
    component::{ComponentId, Components, StorageType},
    entity::Entity,
    query::{
        ArchetypeQueryData, ContiguousQueryData, EcsAccessType, FilteredAccess, IterQueryData,
        QueryData, ReadOnlyQueryData, ReleaseStateQueryData, SingleEntityQueryData, WorldQuery,
    },
    storage::{Table, TableRow},
    world::{World, unsafe_world_cell::UnsafeWorldCell},
};
use bevy_utils::prelude::DebugName;
use std::{iter, marker::PhantomData};

/// Returns a bool that describes if an entity has the component `T`.
///
/// This can be used in a [`Query`] if you want to know whether or not entities
/// have the component `T`  but don't actually care about the component's value.
///
/// # Footguns
///
/// Note that a `Query<Has<T>>` will match all existing entities.
/// Beware! Even if it matches all entities, it doesn't mean that `query.get(entity)`
/// will always return `Ok(bool)`.
///
/// In the case of a non-existent entity, such as a despawned one, it will return `Err`.
/// A workaround is to replace `query.get(entity).unwrap()` by
/// `query.get(entity).unwrap_or_default()`.
///
/// # Examples
///
/// ```
/// # use bevy_ecs::component::Component;
/// # use bevy_ecs::query::Has;
/// # use bevy_ecs::system::IntoSystem;
/// # use bevy_ecs::system::Query;
/// #
/// # #[derive(Component)]
/// # struct IsHungry;
/// # #[derive(Component)]
/// # struct Name { name: &'static str };
/// #
/// fn food_entity_system(query: Query<(&Name, Has<IsHungry>) >) {
///     for (name, is_hungry) in &query {
///         if is_hungry{
///             println!("{} would like some food.", name.name);
///         } else {
///             println!("{} has had sufficient.", name.name);
///         }
///     }
/// }
/// # bevy_ecs::system::assert_is_system(food_entity_system);
/// ```
///
/// ```
/// # use bevy_ecs::component::Component;
/// # use bevy_ecs::query::Has;
/// # use bevy_ecs::system::IntoSystem;
/// # use bevy_ecs::system::Query;
/// #
/// # #[derive(Component)]
/// # struct Alpha{has_beta: bool};
/// # #[derive(Component)]
/// # struct Beta { has_alpha: bool };
/// #
/// // Unlike `Option<&T>`, `Has<T>` is compatible with `&mut T`
/// // as it does not actually access any data.
/// fn alphabet_entity_system(mut alphas: Query<(&mut Alpha, Has<Beta>)>, mut betas: Query<(&mut Beta, Has<Alpha>)>) {
///     for (mut alpha, has_beta) in alphas.iter_mut() {
///         alpha.has_beta = has_beta;
///     }
///     for (mut beta, has_alpha) in betas.iter_mut() {
///         beta.has_alpha = has_alpha;
///     }
/// }
/// # bevy_ecs::system::assert_is_system(alphabet_entity_system);
/// ```
pub struct HasDef<T, const N: usize>(PhantomData<T>);

impl<T, const N: usize> core::fmt::Debug for HasDef<T, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
        write!(f, "Has<{}>", DebugName::type_name::<T>())
    }
}

// SAFETY:
// `update_component_access` does nothing.
// This is sound because `fetch` does not access components.
unsafe impl<T: DefComponent, const N: usize> WorldQuery for HasDef<T, N> {
    type Fetch<'w> = bool;
    type State = ComponentId;

    fn shrink_fetch<'wlong: 'wshort, 'wshort>(fetch: Self::Fetch<'wlong>) -> Self::Fetch<'wshort> {
        fetch
    }

    #[inline]
    unsafe fn init_fetch<'w, 's>(
        _world: UnsafeWorldCell<'w>,
        _state: &'s Self::State,
        _last_run: Tick,
        _this_run: Tick,
    ) -> Self::Fetch<'w> {
        false
    }

    const IS_DENSE: bool = {
        match T::STORAGE_TYPE {
            StorageType::Table => true,
            StorageType::SparseSet => false,
        }
    };

    #[inline]
    unsafe fn set_archetype<'w, 's>(
        fetch: &mut Self::Fetch<'w>,
        state: &'s Self::State,
        archetype: &'w Archetype,
        _table: &Table,
    ) {
        *fetch = archetype.contains(*state);
    }

    #[inline]
    unsafe fn set_table<'w, 's>(
        fetch: &mut Self::Fetch<'w>,
        state: &'s Self::State,
        table: &'w Table,
    ) {
        *fetch = table.has_column(*state);
    }

    fn update_component_access(&component_id: &Self::State, access: &mut FilteredAccess) {
        access.access_mut().add_archetypal(component_id);
    }

    fn init_state(world: &mut World) -> ComponentId {
        DefineRegister::arg_component::<T, N>(world).0
    }

    fn get_state(_components: &Components) -> Option<Self::State> {
        None
    }

    fn matches_component_set(
        _state: &Self::State,
        _set_contains_id: &impl Fn(ComponentId) -> bool,
    ) -> bool {
        // `Has<T>` always matches
        true
    }
}

// SAFETY: `Self` is the same as `Self::ReadOnly`
unsafe impl<T: DefComponent, const N: usize> QueryData for HasDef<T, N> {
    const IS_READ_ONLY: bool = true;
    const IS_ARCHETYPAL: bool = true;
    type ReadOnly = Self;
    type Item<'w, 's> = bool;

    fn shrink<'wlong: 'wshort, 'wshort, 's>(
        item: Self::Item<'wlong, 's>,
    ) -> Self::Item<'wshort, 's> {
        item
    }

    #[inline(always)]
    unsafe fn fetch<'w, 's>(
        _state: &'s Self::State,
        fetch: &mut Self::Fetch<'w>,
        _entity: Entity,
        _table_row: TableRow,
    ) -> Option<Self::Item<'w, 's>> {
        Some(*fetch)
    }

    fn iter_access(_state: &Self::State) -> impl Iterator<Item = EcsAccessType<'_>> {
        iter::empty()
    }
}

// SAFETY: access is read only and only on the current entity
unsafe impl<T: DefComponent, const N: usize> IterQueryData for HasDef<T, N> {}

// SAFETY: access is read only
unsafe impl<T: DefComponent, const N: usize> ReadOnlyQueryData for HasDef<T, N> {}

// SAFETY: access is only on the current entity
unsafe impl<T: DefComponent, const N: usize> SingleEntityQueryData for HasDef<T, N> {}

impl<T: DefComponent, const N: usize> ReleaseStateQueryData for HasDef<T, N> {
    fn release_state<'w>(item: Self::Item<'w, '_>) -> Self::Item<'w, 'static> {
        item
    }
}

impl<T: DefComponent, const N: usize> ArchetypeQueryData for HasDef<T, N> {}

impl<T: DefComponent, const N: usize> ContiguousQueryData for HasDef<T, N> {
    type Contiguous<'w, 's> = bool;

    unsafe fn fetch_contiguous<'w, 's>(
        _state: &'s Self::State,
        fetch: &mut Self::Fetch<'w>,
        _entities: &'w [Entity],
    ) -> Self::Contiguous<'w, 's> {
        *fetch
    }
}
