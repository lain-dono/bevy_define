use super::{DefResMut, DefResource, DefineRegister};
use bevy_ecs::{
    change_detection::{ComponentTickCells, DetectChanges, MaybeLocation, Tick},
    component::{ComponentId, Mutable, StorageType},
    entity::{Entity, EntityLocation},
    query::{DebugCheckedUnwrap as _, FilteredAccess, FilteredAccessSet},
    resource::IS_RESOURCE,
    storage::{ComponentSparseSet, Table},
    system::{ReadOnlySystemParam, SystemMeta, SystemParam, SystemParamValidationError},
    world::{World, unsafe_world_cell::UnsafeWorldCell},
};
use bevy_ptr::Ptr;
use bevy_utils::DebugName;
use core::{ops::Deref, panic::Location};

/// Used by immutable query parameters (such as [`Ref`] and [`Res`])
/// to store immutable access to the [`Tick`]s of a single component or resource.
#[derive(Clone, Copy)]
pub(crate) struct ComponentTicksRef<'w> {
    pub(crate) added: &'w Tick,
    pub(crate) changed: &'w Tick,
    pub(crate) changed_by: MaybeLocation<&'w &'static Location<'static>>,
    pub(crate) last_run: Tick,
    pub(crate) this_run: Tick,
}

/*
impl<'w> ComponentTicksRef<'w> {
    /// # Safety
    /// This should never alias the underlying ticks with a mutable one such as `ComponentTicksMut`.
    #[inline]
    pub(crate) unsafe fn from_tick_cells(
        cells: ComponentTickCells<'w>,
        last_run: Tick,
        this_run: Tick,
    ) -> Self {
        Self {
            // SAFETY: Caller ensures there is no mutable access to the cell.
            added: unsafe { cells.added.deref() },
            // SAFETY: Caller ensures there is no mutable access to the cell.
            changed: unsafe { cells.changed.deref() },
            // SAFETY: Caller ensures there is no mutable access to the cell.
            changed_by: unsafe { cells.changed_by.map(|changed_by| changed_by.deref()) },
            last_run,
            this_run,
        }
    }
}
    */

/// Shared borrow of a [`Resource`].
///
/// See the [`Resource`] documentation for usage.
///
/// If you need a unique mutable borrow, use [`ResMut`] instead.
///
/// This [`SystemParam`](crate::system::SystemParam) fails validation if resource doesn't exist.
/// This will cause a panic, but can be configured to do nothing or warn once.
///
/// Use [`Option<Res<T>>`] instead if the resource might not always exist.
pub struct DefRes<'w, T: ?Sized + DefResource, const N: usize = 0> {
    pub(crate) value: &'w T,
    pub(crate) ticks: ComponentTicksRef<'w>,
}

impl<'w, T: DefResource, const N: usize> DefRes<'w, T, N> {
    /// Copies a reference to a resource.
    ///
    /// Note that unless you actually need an instance of `Res<T>`, you should
    /// prefer to just convert it to `&T` which can be freely copied.
    #[expect(
        clippy::should_implement_trait,
        reason = "As this struct derefs to the inner resource, a `Clone` trait implementation would interfere with the common case of cloning the inner content."
    )]
    pub fn clone(this: &Self) -> Self {
        Self {
            value: this.value,
            ticks: this.ticks,
        }
    }

    /// Due to lifetime limitations of the `Deref` trait, this method can be used to obtain a
    /// reference of the [`Resource`] with a lifetime bound to `'w` instead of the lifetime of the
    /// struct itself.
    pub fn into_inner(self) -> &'w T {
        self.value
    }
}

impl<'w, T: DefResource<Mutability = Mutable>, const N: usize> From<DefResMut<'w, T, N>>
    for DefRes<'w, T, N>
{
    fn from(res: DefResMut<'w, T, N>) -> Self {
        Self {
            value: res.value,
            ticks: res.ticks.into(),
        }
    }
}

/*
impl<'w, T: Resource> From<Res<'w, T>> for Ref<'w, T> {
    /// Convert a `Res` into a `Ref`. This allows keeping the change-detection feature of `Ref`
    /// while losing the specificity of `Res` for resources.
    fn from(res: Res<'w, T>) -> Self {
        Self {
            value: res.value,
            ticks: res.ticks,
        }
    }
}
    */

impl<'a, T: DefResource, const N: usize> IntoIterator for &'a DefRes<'_, T, N>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.value.into_iter()
    }
}
impl<T: ?Sized + DefResource, const N: usize> DetectChanges for DefRes<'_, T, N> {
    #[inline]
    fn is_added(&self) -> bool {
        self.is_added_after(self.ticks.last_run)
    }
    #[inline]
    fn is_changed(&self) -> bool {
        self.is_changed_after(self.ticks.last_run)
    }
    #[inline]
    fn is_added_after(&self, other: Tick) -> bool {
        self.ticks.added.is_newer_than(other, self.ticks.this_run)
    }
    #[inline]
    fn is_changed_after(&self, other: Tick) -> bool {
        self.ticks.changed.is_newer_than(other, self.ticks.this_run)
    }
    #[inline]
    fn last_changed(&self) -> Tick {
        *self.ticks.changed
    }
    #[inline]
    fn added(&self) -> Tick {
        *self.ticks.added
    }
    #[inline]
    fn changed_by(&self) -> MaybeLocation {
        self.ticks.changed_by.copied()
    }
}
impl<T: ?Sized + DefResource, const N: usize> Deref for DefRes<'_, T, N> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.value
    }
}
impl<T: DefResource, const N: usize> AsRef<T> for DefRes<'_, T, N> {
    #[inline]
    fn as_ref(&self) -> &T {
        self.deref()
    }
}
impl<T: ?Sized + DefResource, const N: usize> core::fmt::Debug for DefRes<'_, T, N>
where
    T: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple(stringify!(DefRes))
            .field(&self.value)
            .finish()
    }
}

// SAFETY: Res only reads a single World resource
unsafe impl<T: DefResource, const N: usize> ReadOnlySystemParam for DefRes<'_, T, N> {}

// SAFETY: Res ComponentId access is applied to SystemMeta. If this Res
// conflicts with any prior access, a panic will occur.
unsafe impl<T: DefResource, const N: usize> SystemParam for DefRes<'_, T, N> {
    type State = ComponentId;
    type Item<'w, 's> = DefRes<'w, T, N>;

    fn init_state(world: &mut World) -> Self::State {
        DefineRegister::<T::Define>::arg_scope::<_, N>(world, |world, key, mut def| {
            def.resource::<T>(world, key.clone())
        })
    }

    fn init_access(
        &component_id: &Self::State,
        system_meta: &mut SystemMeta,
        component_access_set: &mut FilteredAccessSet,
        world: &mut World,
    ) {
        let mut filter = FilteredAccess::default();
        filter.add_read(component_id);
        filter.and_with(IS_RESOURCE);

        let conflicts = component_access_set.get_conflicts_single(&filter);
        if conflicts.is_empty() {
            component_access_set.add(filter);
            return;
        }

        // let mut accesses = conflicts.format_conflict_list(world.as_unsafe_world_cell());
        // // Access list may be empty (if access to all components requested)
        // if !accesses.is_empty() {
        //     accesses.push(' ');
        // }
        panic!(
            "error[B0002]: Res<{}> in system {} conflicts with a previous system parameter. Consider removing the duplicate access using `Without<IsResource>` to create disjoint Queries or merging conflicting Queries into a `ParamSet`. See: https://bevy.org/learn/errors/b0002",
            DebugName::type_name::<T>(),
            system_meta.name()
        );
    }

    #[inline]
    unsafe fn get_param<'w, 's>(
        &mut component_id: &'s mut Self::State,
        system_meta: &SystemMeta,
        world: UnsafeWorldCell<'w>,
        change_tick: Tick,
    ) -> Result<Self::Item<'w, 's>, SystemParamValidationError> {
        let (ptr, ticks) = get_resource_with_ticks(world, component_id).ok_or_else(|| {
            SystemParamValidationError::invalid::<Self>("Resource does not exist")
        })?;
        Ok(DefRes {
            value: ptr.deref(),
            ticks: ComponentTicksRef {
                // added: ticks.added.deref(),
                //changed: ticks.changed.deref(),
                //changed_by: ticks.changed_by.map(|changed_by| changed_by.deref()),
                added: ticks.added.get().as_ref_unchecked(),
                changed: ticks.changed.get().as_ref_unchecked(),
                changed_by: ticks
                    .changed_by
                    .map(|changed_by| changed_by.get().as_ref_unchecked()),
                last_run: system_meta.get_last_run(),
                this_run: change_tick,
            },
        })
    }
}

// Shorthand helper function for getting the data and change ticks for a resource.
/// # Safety
/// It is the caller's responsibility to ensure that
/// - the [`UnsafeWorldCell`] has permission to access the resource mutably
/// - no mutable references to the resource exist at the same time
#[inline]
pub(crate) unsafe fn get_resource_with_ticks(
    world: UnsafeWorldCell<'_>,
    component_id: ComponentId,
) -> Option<(Ptr<'_>, ComponentTickCells<'_>)> {
    // SAFETY: We have permission to access the resource of `component_id`.
    let entity = unsafe { world.resource_entities() }.get(component_id)?;
    let storage_type = world.components().get_info(component_id)?.storage_type();
    let location = world.get_entity(entity).ok()?.location();
    // SAFETY:
    // - caller ensures there is no `&mut World`
    // - caller ensures there are no mutable borrows of this resource
    // - caller ensures that we have permission to access this resource
    // - storage_type and location are valid
    get_component_and_ticks(world, component_id, storage_type, entity, location)
}

/// Get an untyped pointer to a particular [`Component`] and its [`ComponentTicks`]
///
/// # Safety
/// - `location` must refer to an archetype that contains `entity`
/// - `component_id` must be valid
/// - `storage_type` must accurately reflect where the components for `component_id` are stored.
/// - the caller must ensure that no aliasing rules are violated
#[inline]
unsafe fn get_component_and_ticks(
    world: UnsafeWorldCell<'_>,
    component_id: ComponentId,
    storage_type: StorageType,
    entity: Entity,
    location: EntityLocation,
) -> Option<(Ptr<'_>, ComponentTickCells<'_>)> {
    match storage_type {
        StorageType::Table => {
            let table = fetch_table(world, location)?;

            // SAFETY: archetypes only store valid table_rows and caller ensure aliasing rules
            Some((
                table.get_component(component_id, location.table_row)?,
                ComponentTickCells {
                    added: table
                        .get_added_tick(component_id, location.table_row)
                        .debug_checked_unwrap(),
                    changed: table
                        .get_changed_tick(component_id, location.table_row)
                        .debug_checked_unwrap(),
                    changed_by: table
                        .get_changed_by(component_id, location.table_row)
                        .map(|changed_by| changed_by.debug_checked_unwrap()),
                },
            ))
        }
        StorageType::SparseSet => fetch_sparse_set(world, component_id)?.get_with_ticks(entity),
    }
}

#[inline]
/// # Safety
/// - the returned `Table` is only used in ways that this [`UnsafeWorldCell`] has permission for.
/// - the returned `Table` is only used in ways that would not conflict with any existing borrows of world data.
unsafe fn fetch_table(world: UnsafeWorldCell<'_>, location: EntityLocation) -> Option<&Table> {
    // SAFETY:
    // - caller ensures returned data is not misused and we have not created any borrows of component/resource data
    // - `location` contains a valid `TableId`, so getting the table won't fail
    unsafe { world.storages().tables.get(location.table_id) }
}

#[inline]
/// # Safety
/// - the returned `ComponentSparseSet` is only used in ways that this [`UnsafeWorldCell`] has permission for.
/// - the returned `ComponentSparseSet` is only used in ways that would not conflict with any existing
///   borrows of world data.
unsafe fn fetch_sparse_set(
    world: UnsafeWorldCell<'_>,
    component_id: ComponentId,
) -> Option<&ComponentSparseSet> {
    // SAFETY: caller ensures returned data is not misused and we have not created any borrows
    // of component/resource data
    unsafe { world.storages() }.sparse_sets.get(component_id)
}
