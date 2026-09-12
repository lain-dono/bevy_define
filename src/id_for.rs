use super::{DefComponent, DefKey, DefineRegister, Key};
use bevy_ecs::{
    component::ComponentId,
    system::{Local, SystemParam},
    world::{FromWorld, Mut, World},
};
use std::{any::TypeId, marker::PhantomData, ops::Deref};

/// A [`SystemParam`] that provides access to the [`ComponentId`] for a specific component type.
///
/// # Example
/// ```
/// # use bevy_ecs::{system::Local, component::{Component, ComponentId, ComponentIdFor}};
/// #[derive(Component)]
/// struct Player;
/// fn my_system(component_id: ComponentIdFor<Player>) {
///     let component_id: ComponentId = component_id.get();
///     // ...
/// }
/// ```
#[derive(SystemParam)]
pub struct DefComponentIdFor<'s, T: DefComponent, const N: usize = 0>(
    Local<'s, InitComponentId<T, N>>,
);

impl<T: DefComponent, const N: usize> DefComponentIdFor<'_, T, N> {
    /// Gets the [`ComponentId`] for the type `T`.
    #[inline]
    pub fn get(&self) -> ComponentId {
        **self
    }

    #[inline]
    pub fn key(&self) -> &Key {
        &self.0.key
    }

    #[inline]
    pub fn type_id(&self) -> TypeId {
        self.0.type_id
    }
}

impl<T: DefComponent, const N: usize> Deref for DefComponentIdFor<'_, T, N> {
    type Target = ComponentId;
    fn deref(&self) -> &Self::Target {
        &self.0.component_id
    }
}

impl<T: DefComponent, const N: usize> From<DefComponentIdFor<'_, T, N>> for ComponentId {
    #[inline]
    fn from(to_component_id: DefComponentIdFor<T, N>) -> ComponentId {
        *to_component_id
    }
}

/// Initializes the [`ComponentId`] for a specific type when used with [`FromWorld`].
struct InitComponentId<T: DefComponent, const N: usize> {
    key: Key,
    component_id: ComponentId,
    type_id: TypeId,
    marker: PhantomData<T>,
}

impl<T: DefComponent, const N: usize> FromWorld for InitComponentId<T, N> {
    fn from_world(world: &mut World) -> Self {
        let (key, (component_id, type_id)) =
            world.resource_scope(|world, key: Mut<'_, DefKey<N>>| {
                world.resource_scope(|world, mut def: Mut<'_, DefineRegister>| {
                    (key.0.clone(), def.component::<T>(world, key.0.clone()))
                })
            });

        Self {
            key,
            component_id,
            type_id,
            marker: PhantomData,
        }
    }
}
