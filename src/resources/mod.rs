mod res;
mod res_mut;

use super::{Define, DefineRegister};
use bevy_ecs::change_detection::MaybeLocation;
use bevy_ecs::component::{
    ComponentCloneBehavior, ComponentDescriptor, ComponentId, ComponentMutability, StorageType,
};
use bevy_ecs::world::World;
use bevy_ptr::OwningPtr;
use std::{any::TypeId, borrow::Cow, hash::Hash};

pub use self::res::DefRes;
pub use self::res_mut::DefResMut;

/// # Safety
///
/// [`DefResource::INDEX`] must refer to a position within [`Define::Resources`].
pub unsafe trait DefResource: Send + Sync + 'static {
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

pub trait DefineResources<Key: Send + Sync + Eq + Hash> {
    fn register(world: &mut World, key: &Key) -> Box<[(ComponentId, TypeId)]>;
}

impl<Key: Send + Sync + Eq + Hash> DefineResources<Key> for () {
    fn register(_world: &mut World, _key: &Key) -> Box<[(ComponentId, TypeId)]> {
        vec![].into()
    }
}

macro_rules! impl_define_resources {
    ($($T:ident),*) => {
        impl<Def: Define, $($T: DefResource<Define=Def>),*> DefineResources<Def::Key> for ($($T,)*) {
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

variadics_please::all_tuples!(impl_define_resources, 1, 15, T);

#[track_caller]
pub fn insert_resource<T: DefResource>(world: &mut World, key: <T::Define as Define>::Key, val: T) {
    insert_resource_with_caller(world, key, val, MaybeLocation::caller());
}

pub fn insert_resource_with_caller<T: DefResource>(
    world: &mut World,
    key: <T::Define as Define>::Key,
    val: T,
    caller: MaybeLocation,
) {
    let id = DefineRegister::world_scope_resource::<T>(world, key);
    unsafe { OwningPtr::make(val, |value| world.insert_resource_by_id(id, value, caller)) };
}

pub fn get_resource<T: DefResource>(
    world: &mut World,
    key: <T::Define as Define>::Key,
) -> Option<&T> {
    let id = DefineRegister::world_scope_resource::<T>(world, key);
    unsafe { world.get_resource_by_id(id).map(|res| res.deref::<T>()) }
}
