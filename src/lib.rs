#![expect(unsafe_code)]
#![expect(unsafe_op_in_unsafe_fn)]
#![expect(clippy::type_complexity)]
#![expect(clippy::needless_lifetimes)]

use bevy_ecs::{
    component::{ComponentCloneBehavior, ComponentId},
    entity::{ComponentCloneCtx, SourceComponent},
    prelude::*,
};
use bevy_platform::collections::HashMap;
use bevy_ptr::OwningPtr;
use std::{hash::Hash, marker::PhantomData};

mod components;
mod resources;

pub use self::components::{
    def::{Def, DefReadFetch},
    def_has::HasDef,
    def_mut::{DefMut, DefWriteFetch},
    def_ref::{DefRef, DefRefFetch},
    id_for::DefComponentIdFor,
    {DefComponent, DefineComponents, EntityDef},
};
pub use self::resources::{
    DefRes, DefResMut, DefResource, DefineResources, get_resource, insert_resource,
};

type Slot = (Box<[ComponentId]>, Box<[ComponentId]>);

#[derive(Resource)]
pub struct DefineRegister<Marker: Define> {
    slots: HashMap<Marker::Key, Slot>,
    marker: PhantomData<Marker>,
}

impl<Marker: Define> Default for DefineRegister<Marker> {
    fn default() -> Self {
        Self {
            slots: HashMap::default(),
            marker: PhantomData,
        }
    }
}

impl<Marker: Define> DefineRegister<Marker> {
    pub fn component<T: DefComponent<Define = Marker>>(
        &mut self,
        world: &mut World,
        key: Marker::Key,
    ) -> ComponentId {
        self.get_or_init(world, key).0[T::INDEX]
    }

    pub fn resource<T: DefResource<Define = Marker>>(
        &mut self,
        world: &mut World,
        key: Marker::Key,
    ) -> ComponentId {
        self.get_or_init(world, key).1[T::INDEX]
    }

    fn get_or_init(&mut self, world: &mut World, key: Marker::Key) -> &mut Slot {
        self.slots.entry(key).or_insert_with_key(|key| {
            (
                Marker::Components::register(world, key),
                Marker::Resources::register(world, key),
            )
        })
    }

    fn arg_scope<R, const N: usize>(
        world: &mut World,
        f: impl FnOnce(&mut World, &Marker::Key, Mut<'_, Self>) -> R,
    ) -> R {
        world.resource_scope(|world, key: Mut<'_, DefKey<Marker::Key, N>>| {
            world.resource_scope(|world, def: Mut<'_, Self>| f(world, &key.0, def))
        })
    }

    fn entity_scope_component<T: DefComponent<Define = Marker>>(
        entity: &mut EntityWorldMut,
        key: Marker::Key,
    ) -> ComponentId {
        entity.resource_scope(|entity, mut def: Mut<Self>| unsafe {
            def.component::<T>(entity.world_mut(), key)
        })
    }

    fn world_scope_resource<T: DefResource<Define = Marker>>(
        world: &mut World,
        key: Marker::Key,
    ) -> ComponentId {
        world.resource_scope(|world, mut def: Mut<Self>| unsafe { def.resource::<T>(world, key) })
    }
}

#[derive(Resource)]
pub struct DefKey<Key: Clone + Send + Sync + Eq + Hash, const N: usize = 0>(pub Key);

impl<Key: Clone + Send + Sync + Eq + Hash, const N: usize> Clone for DefKey<Key, N> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<Key: Clone + Send + Sync + Eq + Hash, const N: usize> DefKey<Key, N> {
    pub fn arg<const M: usize>(&self) -> DefKey<Key, M> {
        DefKey(self.0.clone())
    }

    pub fn key(&self) -> Key {
        self.0.clone()
    }
}

impl<Key: Clone + Send + Sync + Eq + Hash, const N: usize> std::ops::Deref for DefKey<Key, N> {
    type Target = Key;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub trait Define: Send + Sync + 'static {
    type Key: Clone + Send + Sync + Eq + Hash;
    type Components: DefineComponents<Self::Key>;
    type Resources: DefineResources<Self::Key>;
}

unsafe fn drop_fn_for<T: 'static>() -> Option<for<'a> unsafe fn(OwningPtr<'a>)> {
    /// # Safety
    ///
    /// `x` must point to a valid value of type `T`.
    #[expect(unsafe_code)]
    unsafe fn drop_ptr<T>(x: OwningPtr<'_>) {
        // SAFETY: Contract is required to be upheld by the caller.
        unsafe { x.drop_as::<T>() }
    }

    core::mem::needs_drop::<T>().then_some(drop_ptr::<T> as _)
}

pub fn clone_def<T: Clone>() -> ComponentCloneBehavior {
    ComponentCloneBehavior::Custom(
        |source: &SourceComponent<'_>, ctx: &mut ComponentCloneCtx<'_, '_>| {
            // Safety: no
            unsafe {
                let val = source.ptr().deref::<T>().clone();
                OwningPtr::make(val, |ptr| ctx.write_target_component_ptr(ptr.as_ref()));
            }
        },
    )
}

pub fn copy_def<T: Copy>() -> ComponentCloneBehavior {
    ComponentCloneBehavior::Custom(
        |source: &SourceComponent<'_>, ctx: &mut ComponentCloneCtx<'_, '_>| {
            // Safety: no
            unsafe { ctx.write_target_component_ptr(source.ptr()) };
        },
    )
}
