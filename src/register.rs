use crate::{DefComponent, DefKey, Key};
use bevy_ecs::{
    component::{ComponentDescriptor, ComponentId, ComponentMutability},
    resource::Resource,
    world::{
        EntityWorldMut, Mut, World,
        unsafe_world_cell::{UnsafeEntityCell, UnsafeWorldCell},
    },
};
use bevy_platform::collections::{HashMap, hash_map::Keys};
use bevy_ptr::OwningPtr;
use std::any::TypeId;

#[derive(Resource, Default)]
pub struct DefineRegister {
    slots: HashMap<(Key, TypeId), ComponentId>,
    index: HashMap<ComponentId, (Key, TypeId)>,
    types: HashMap<TypeId, Vec<ComponentId>>,
}

impl DefineRegister {
    pub fn find(&self, id: ComponentId) -> Option<(DefKey, TypeId)> {
        self.index.get(&id).map(|(k, id)| (DefKey(k.clone()), *id))
    }

    pub fn keys(&self) -> Keys<'_, (Key, TypeId), ComponentId> {
        self.slots.keys()
    }

    pub fn types<T: DefComponent>(&self) -> &Vec<ComponentId> {
        self.types.get(&TypeId::of::<T>()).unwrap()
    }

    pub fn component<T>(&mut self, world: &mut World, key: Key) -> (ComponentId, TypeId)
    where
        T: DefComponent,
    {
        self.get_or_insert_component::<T>(world.as_unsafe_world_cell(), key)
    }

    fn get_or_insert_component<T: DefComponent>(
        &mut self,
        world: UnsafeWorldCell<'_>,
        key: Key,
    ) -> (ComponentId, TypeId) {
        /// # Safety
        ///
        /// `x` must point to a valid value of type `T`.
        #[expect(unsafe_code)]
        unsafe fn drop_ptr<T>(x: OwningPtr<'_>) {
            // SAFETY: Contract is required to be upheld by the caller.
            unsafe { x.drop_as::<T>() }
        }

        let type_id = TypeId::of::<T>();
        let component_id =
            *(self.slots.entry((key, type_id))).or_insert_with_key(|&(ref key, type_id)| {
                let world = unsafe { world.world_mut() };

                // SAFETY: `T` is a rust type, so the layout will have `size()` as a multiple of `align()`
                let id = world.register_component_with_descriptor(unsafe {
                    ComponentDescriptor::new_with_layout(
                        T::name(key),
                        T::STORAGE_TYPE,
                        core::alloc::Layout::new::<T>(),
                        core::mem::needs_drop::<T>().then_some(drop_ptr::<T> as _),
                        T::Mutability::MUTABLE,
                        T::clone_behavior(),
                        None,
                    )
                });

                self.index.insert(id, (key.clone(), type_id));

                let ty = self.types.entry(type_id).or_default();
                ty.push(id);

                id
            });

        (component_id, type_id)
    }

    pub(crate) fn arg_component<T: DefComponent, const N: usize>(
        world: &mut World,
    ) -> (ComponentId, TypeId) {
        world.resource_scope(|world, key: Mut<'_, DefKey<N>>| {
            world.resource_scope(|world, mut def: Mut<'_, Self>| {
                def.component::<T>(world, key.key())
            })
        })
    }

    pub(crate) fn entity_component<T: DefComponent>(
        entity: &mut EntityWorldMut,
        key: Key,
    ) -> (ComponentId, TypeId) {
        entity.resource_scope(|entity, mut def: Mut<Self>| {
            entity.world_scope(|world| def.component::<T>(world, key))
        })
    }

    pub(crate) unsafe fn cell_component<T: DefComponent>(
        cell: UnsafeEntityCell<'_>,
        key: Key,
    ) -> Option<(ComponentId, TypeId)> {
        let mut def = unsafe { cell.world().get_resource_mut::<Self>()? };
        Some(def.get_or_insert_component::<T>(cell.world(), key))
    }
}
