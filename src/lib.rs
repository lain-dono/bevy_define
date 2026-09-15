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

mod def;
mod def_mut;
mod def_ref;
mod fetch;
mod has;
mod id_for;
mod key;
mod reflect;
mod register;

pub use self::{
    def::Def,
    has::HasDef,
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
