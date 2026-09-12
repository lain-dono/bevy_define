#![expect(unsafe_code)]
#![expect(unsafe_op_in_unsafe_fn)]
#![expect(clippy::type_complexity)]
#![expect(clippy::needless_lifetimes)]

use bevy_ecs::{
    component::{ComponentCloneBehavior, ComponentMutability, StorageType},
    entity::{ComponentCloneCtx, SourceComponent},
    system::{EntityCommand, EntityCommands},
    world::EntityWorldMut,
};
use bevy_ptr::OwningPtr;
use std::borrow::Cow;
use std::marker::PhantomData;

mod def;
mod def_has;
mod def_mut;
mod def_ref;
mod id_for;
mod key;
mod reflect;
mod register;

pub use self::{
    def::{Def, DefReadFetch},
    def_has::HasDef,
    def_mut::{DefMut, DefWriteFetch},
    def_ref::{DefRef, DefRefFetch},
    id_for::DefComponentIdFor,
    key::{DefKey, Key, KeyDisplay},
    reflect::ReflectDef,
    register::DefineRegister,
};

pub fn clone_def<T: Clone>() -> ComponentCloneBehavior {
    ComponentCloneBehavior::Custom(
        // Safety: no
        |source: &SourceComponent<'_>, ctx: &mut ComponentCloneCtx<'_, '_>| unsafe {
            let val = source.ptr().deref::<T>().clone();
            OwningPtr::make(val, |ptr| ctx.write_target_component_ptr(ptr.as_ref()));
        },
    )
}

pub fn copy_def<T: Copy>() -> ComponentCloneBehavior {
    ComponentCloneBehavior::Custom(
        // Safety: no
        |source: &SourceComponent<'_>, ctx: &mut ComponentCloneCtx<'_, '_>| unsafe {
            ctx.write_target_component_ptr(source.ptr())
        },
    )
}

/// # Safety
///
/// [`DefComponent::INDEX`] must refer to a position within [`Define::Components`].
pub unsafe trait DefComponent: Send + Sync + 'static {
    type Mutability: ComponentMutability;

    const STORAGE_TYPE: StorageType = StorageType::Table;

    #[inline]
    fn name(_key: &Key) -> Cow<'static, str> {
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

pub trait EntityDef {
    fn insert_def<T: DefComponent>(&mut self, key: Key, val: T) -> &mut Self;
    fn remove_def<T: DefComponent>(&mut self, key: Key) -> &mut Self;
}

impl EntityDef for EntityWorldMut<'_> {
    fn insert_def<T: DefComponent>(&mut self, key: Key, val: T) -> &mut Self {
        insert_component::<T>(self, key, val);
        self
    }

    fn remove_def<T: DefComponent>(&mut self, key: Key) -> &mut Self {
        remove_component::<T>(self, key);
        self
    }
}

impl EntityDef for EntityCommands<'_> {
    fn insert_def<T: DefComponent>(&mut self, key: Key, val: T) -> &mut Self {
        self.queue(insert_def(key, val))
    }

    fn remove_def<T: DefComponent>(&mut self, key: Key) -> &mut Self {
        self.queue(remove_def::<T>(key))
    }
}

pub fn insert_def<T: DefComponent>(key: Key, val: T) -> impl EntityCommand {
    move |mut entity: EntityWorldMut<'_>| insert_component::<T>(&mut entity, key, val)
}

pub fn remove_def<T: DefComponent>(key: Key) -> impl EntityCommand {
    move |mut entity: EntityWorldMut<'_>| remove_component::<T>(&mut entity, key)
}

fn insert_component<T: DefComponent>(entity: &mut EntityWorldMut<'_>, key: Key, val: T) {
    let (id, _) = DefineRegister::entity_component::<T>(entity, key);
    unsafe { OwningPtr::make(val, |component| entity.insert_by_id(id, component)) };
}

fn remove_component<T: DefComponent>(entity: &mut EntityWorldMut<'_>, key: Key) {
    let (id, _) = DefineRegister::entity_component::<T>(entity, key);
    entity.remove_by_id(id);
}
