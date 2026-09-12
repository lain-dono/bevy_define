pub mod def;
pub mod def_has;
pub mod def_mut;
pub mod def_ref;
pub mod id_for;
pub mod reflect;

use super::{Define, DefineRegister};
use bevy_ecs::component::{
    ComponentCloneBehavior, ComponentDescriptor, ComponentId, ComponentMutability, StorageType,
};
use bevy_ecs::system::{EntityCommand, EntityCommands};
use bevy_ecs::world::{EntityWorldMut, World};
use bevy_ptr::OwningPtr;
use std::any::TypeId;
use std::borrow::Cow;
use std::hash::Hash;
use std::marker::PhantomData;

pub trait DefineComponents<Key: Send + Sync + Eq + Hash> {
    fn register(world: &mut World, key: &Key) -> Box<[(ComponentId, TypeId)]>;
}

impl<Key: Send + Sync + Eq + Hash> DefineComponents<Key> for () {
    fn register(_world: &mut World, _key: &Key) -> Box<[(ComponentId, TypeId)]> {
        vec![].into()
    }
}

macro_rules! impl_define_components {
    ($($T:ident),*) => {
        impl<Def: Define, $($T: DefComponent<Define=Def>),*> DefineComponents<Def::Key> for ($($T,)*) {
            fn register(world: &mut World, key: &Def::Key) -> Box<[(ComponentId, TypeId)]> {
                vec![ $( (
                    // SAFETY: `T` is a rust type, so the layout will have `size()` as a multiple of `align()`
                    world.register_component_with_descriptor(unsafe {
                        ComponentDescriptor::new_with_layout(
                            $T::name(key),
                            $T::STORAGE_TYPE,
                            core::alloc::Layout::new::<$T>(),
                            crate::drop_fn_for::<$T>(),
                            $T::Mutability::MUTABLE,
                            $T::clone_behavior(),
                            None,
                        )
                    }),
                    TypeId::of::<$T>(),
                ) ),* ].into()
            }
        }
    };
}

variadics_please::all_tuples!(impl_define_components, 1, 15, T);

/// # Safety
///
/// [`DefComponent::INDEX`] must refer to a position within [`Define::Components`].
pub unsafe trait DefComponent: Send + Sync + 'static {
    type Define: Define;
    type Mutability: ComponentMutability;

    const STORAGE_TYPE: StorageType = StorageType::Table;
    const INDEX: usize;

    #[inline]
    fn name(_key: &<Self::Define as Define>::Key) -> Cow<'static, str> {
        std::any::type_name::<Self>().into()
    }

    #[inline]
    fn clone_behavior() -> ComponentCloneBehavior {
        ComponentCloneBehavior::Default
    }
}

/// A compile-time checked union of two different types that differs based on the
/// [`StorageType`] of a given component.
#[expect(clippy::default_union_representation)]
pub(super) union StorageSwitch<C: DefComponent, T: Copy, S: Copy> {
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

pub trait EntityDef {
    fn insert_def<T: DefComponent>(&mut self, key: <T::Define as Define>::Key, val: T)
    -> &mut Self;
    fn remove_def<T: DefComponent>(&mut self, key: <T::Define as Define>::Key) -> &mut Self;
}

impl EntityDef for EntityWorldMut<'_> {
    fn insert_def<T: DefComponent>(
        &mut self,
        key: <T::Define as Define>::Key,
        val: T,
    ) -> &mut Self {
        insert_component::<T>(self, key, val);
        self
    }

    fn remove_def<T: DefComponent>(&mut self, key: <T::Define as Define>::Key) -> &mut Self {
        remove_component::<T>(self, key);
        self
    }
}

impl EntityDef for EntityCommands<'_> {
    fn insert_def<T: DefComponent>(
        &mut self,
        key: <T::Define as Define>::Key,
        val: T,
    ) -> &mut Self {
        self.queue(insert_def(key, val))
    }

    fn remove_def<T: DefComponent>(&mut self, key: <T::Define as Define>::Key) -> &mut Self {
        self.queue(remove_def::<T>(key))
    }
}

pub fn insert_def<T: DefComponent>(key: <T::Define as Define>::Key, val: T) -> impl EntityCommand {
    move |mut entity: EntityWorldMut<'_>| insert_component::<T>(&mut entity, key, val)
}

pub fn remove_def<T: DefComponent>(key: <T::Define as Define>::Key) -> impl EntityCommand {
    move |mut entity: EntityWorldMut<'_>| remove_component::<T>(&mut entity, key)
}

fn insert_component<T: DefComponent>(
    entity: &mut EntityWorldMut<'_>,
    key: <T::Define as Define>::Key,
    val: T,
) {
    unsafe {
        let id = DefineRegister::entity_scope_component::<T>(entity, key);
        OwningPtr::make(val, |component| entity.insert_by_id(id, component));
    }
}

fn remove_component<T: DefComponent>(
    entity: &mut EntityWorldMut<'_>,
    key: <T::Define as Define>::Key,
) {
    let id = DefineRegister::entity_scope_component::<T>(entity, key);
    entity.remove_by_id(id);
}
