use super::{DefComponent, DefineRegister, Key};
use bevy_ecs::{
    component::ComponentMutability as _, prelude::*, reflect::from_reflect_with_fallback,
    world::unsafe_world_cell::UnsafeEntityCell,
};
use bevy_ptr::OwningPtr;
use bevy_reflect::{FromType, PartialReflect, Reflect, TypePath, TypeRegistry};
use bevy_utils::DebugName;

#[derive(Clone)]
pub struct ReflectDef {
    pub insert: for<'w> fn(Key, &mut EntityWorldMut<'w>, &dyn PartialReflect, &TypeRegistry),
    pub apply: for<'w> fn(Key, EntityMut<'w>, &dyn PartialReflect),
    pub remove: for<'w> fn(Key, &mut EntityWorldMut<'w>),
    pub reflect: for<'w> unsafe fn(Key, UnsafeEntityCell<'w>) -> Option<Mut<'w, dyn Reflect>>,
}

impl ReflectDef {
    pub fn insert(
        &self,
        key: Key,
        entity: &mut EntityWorldMut<'_>,
        component: &dyn PartialReflect,
        registry: &TypeRegistry,
    ) {
        (self.insert)(key, entity, component, registry);
    }

    pub fn apply(&self, key: Key, entity: EntityMut<'_>, component: &dyn PartialReflect) {
        (self.apply)(key, entity, component);
    }

    pub fn remove(&self, key: Key, entity: &mut EntityWorldMut<'_>) {
        (self.remove)(key, entity);
    }

    pub unsafe fn reflect<'w>(
        &self,
        key: Key,
        entity: UnsafeEntityCell<'w>,
    ) -> Option<Mut<'w, dyn Reflect>> {
        unsafe { (self.reflect)(key, entity) }
    }
}

impl<T: DefComponent + Reflect + TypePath> FromType<T> for ReflectDef {
    fn from_type() -> Self {
        Self {
            insert: |key, entity, component, registry| unsafe {
                let (id, _) = DefineRegister::entity_component::<T>(entity, key);

                let component = entity.world_scope(|world| {
                    from_reflect_with_fallback::<T>(component, world, registry)
                });

                OwningPtr::make(component, |component| entity.insert_by_id(id, component));
            },

            apply: |key, mut entity, reflected_component| unsafe {
                if !T::Mutability::MUTABLE {
                    let name = DebugName::type_name::<T>();
                    let name = name.shortname();
                    panic!(
                        "Cannot call `ReflectDef::apply` on component {name}. It is immutable, and cannot modified through reflection"
                    );
                }

                // SAFETY: guard ensures `T` is a mutable component

                let cell = entity.as_unsafe_entity_cell();

                let Some((component_id, _)) = DefineRegister::cell_component::<T>(cell, key) else {
                    return;
                };
                let Ok(ptr) = cell.get_mut_by_id(component_id) else {
                    return;
                };
                let mut component = ptr.map_unchanged(|ptr| ptr.deref_mut::<T>());
                component.apply(reflected_component);
            },
            remove: |key, entity| unsafe {
                let component_id = DefineRegister::cell_component::<T>(
                    entity.as_mutable().as_unsafe_entity_cell(),
                    key,
                );
                if let Some((component_id, _)) = component_id {
                    entity.remove_by_id(component_id);
                }
            },
            reflect: |key, cell| unsafe {
                let (component_id, _) = DefineRegister::cell_component::<T>(cell, key)?;
                let ptr = cell.get_mut_by_id(component_id).ok()?;
                Some(ptr.map_unchanged(|ptr| ptr.deref_mut::<T>().as_reflect_mut()))
            },
        }
    }
}
