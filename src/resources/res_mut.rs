use super::{DefResource, DefineRegister, res::ComponentTicksRef};
use bevy_ecs::{
    change_detection::{DetectChanges, DetectChangesMut, MaybeLocation, MutUntyped, Tick},
    component::{ComponentId, Mutable},
    query::{FilteredAccess, FilteredAccessSet},
    resource::IS_RESOURCE,
    system::{SystemMeta, SystemParam, SystemParamValidationError},
    world::{World, unsafe_world_cell::UnsafeWorldCell},
};
use bevy_ptr::PtrMut;
use bevy_utils::DebugName;
use core::{ops::Deref, ops::DerefMut, panic::Location};

/// Used by mutable query parameters (such as [`Mut`] and [`ResMut`])
/// to store mutable access to the [`Tick`]s of a single component or resource.
pub(crate) struct ComponentTicksMut<'w> {
    pub(crate) added: &'w mut Tick,
    pub(crate) changed: &'w mut Tick,
    pub(crate) changed_by: MaybeLocation<&'w mut &'static Location<'static>>,
    pub(crate) last_run: Tick,
    pub(crate) this_run: Tick,
}

/*
impl<'w> ComponentTicksMut<'w> {
    /// # Safety
    /// This should never alias the underlying ticks. All access must be unique.
    #[inline]
    pub(crate) unsafe fn from_tick_cells(
        cells: ComponentTickCells<'w>,
        last_run: Tick,
        this_run: Tick,
    ) -> Self {
        Self {
            // SAFETY: Caller ensures there is no alias to the cell.
            added: unsafe { cells.added.deref_mut() },
            // SAFETY: Caller ensures there is no alias to the cell.
            changed: unsafe { cells.changed.deref_mut() },
            // SAFETY: Caller ensures there is no alias to the cell.
            changed_by: unsafe { cells.changed_by.map(|changed_by| changed_by.deref_mut()) },
            last_run,
            this_run,
        }
    }
}
    */

impl<'w> From<ComponentTicksMut<'w>> for ComponentTicksRef<'w> {
    fn from(ticks: ComponentTicksMut<'w>) -> Self {
        ComponentTicksRef {
            added: ticks.added,
            changed: ticks.changed,
            changed_by: ticks.changed_by.map(|changed_by| &*changed_by),
            last_run: ticks.last_run,
            this_run: ticks.this_run,
        }
    }
}

/// Unique mutable borrow of a [`Resource`].
///
/// See the [`Resource`] documentation for usage.
///
/// If you need a shared borrow, use [`Res`] instead.
///
/// This [`SystemParam`](crate::system::SystemParam) fails validation if resource doesn't exist.
/// This will cause a panic, but can be configured to do nothing or warn once.
///
/// Use [`Option<ResMut<T>>`] instead if the resource might not always exist.
pub struct DefResMut<'w, T: ?Sized + DefResource<Mutability = Mutable>, const N: usize> {
    pub(crate) value: &'w mut T,
    pub(crate) ticks: ComponentTicksMut<'w>,
}

impl<'a, T: DefResource<Mutability = Mutable>, const N: usize> IntoIterator
    for &'a DefResMut<'_, T, N>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.value.into_iter()
    }
}

impl<'a, T: DefResource<Mutability = Mutable>, const N: usize> IntoIterator
    for &'a mut DefResMut<'_, T, N>
where
    &'a mut T: IntoIterator,
{
    type Item = <&'a mut T as IntoIterator>::Item;
    type IntoIter = <&'a mut T as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.set_changed();
        self.value.into_iter()
    }
}

impl<T: ?Sized + DefResource<Mutability = Mutable>, const N: usize> DetectChanges
    for DefResMut<'_, T, N>
{
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
impl<T: ?Sized + DefResource<Mutability = Mutable>, const N: usize> Deref for DefResMut<'_, T, N> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.value
    }
}
impl<T: DefResource<Mutability = Mutable>, const N: usize> AsRef<T> for DefResMut<'_, T, N> {
    #[inline]
    fn as_ref(&self) -> &T {
        self.deref()
    }
}
impl<T: ?Sized + DefResource<Mutability = Mutable>, const N: usize> DetectChangesMut
    for DefResMut<'_, T, N>
{
    type Inner = T;
    #[inline]
    #[track_caller]
    fn set_changed(&mut self) {
        *self.ticks.changed = self.ticks.this_run;
        self.ticks.changed_by.assign(MaybeLocation::caller());
    }
    #[inline]
    #[track_caller]
    fn set_added(&mut self) {
        *self.ticks.changed = self.ticks.this_run;
        *self.ticks.added = self.ticks.this_run;
        self.ticks.changed_by.assign(MaybeLocation::caller());
    }
    #[inline]
    #[track_caller]
    fn set_last_changed(&mut self, last_changed: Tick) {
        *self.ticks.changed = last_changed;
        self.ticks.changed_by.assign(MaybeLocation::caller());
    }
    #[inline]
    #[track_caller]
    fn set_last_added(&mut self, last_added: Tick) {
        *self.ticks.added = last_added;
        *self.ticks.changed = last_added;
        self.ticks.changed_by.assign(MaybeLocation::caller());
    }
    #[inline]
    fn bypass_change_detection(&mut self) -> &mut Self::Inner {
        self.value
    }
}
impl<T: ?Sized + DefResource<Mutability = Mutable>, const N: usize> DerefMut
    for DefResMut<'_, T, N>
{
    #[inline]
    #[track_caller]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.set_changed();
        self.ticks.changed_by.assign(MaybeLocation::caller());
        self.value
    }
}
impl<T: DefResource<Mutability = Mutable>, const N: usize> AsMut<T> for DefResMut<'_, T, N> {
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        self.deref_mut()
    }
}
impl<'w, T: ?Sized + DefResource<Mutability = Mutable>, const N: usize> DefResMut<'w, T, N> {
    #[doc = r" Consume `self` and return a mutable reference to the"]
    #[doc = r#" contained value while marking `self` as "changed"."#]
    #[inline]
    pub fn into_inner(mut self) -> &'w mut T {
        self.set_changed();
        self.value
    }
    /*
    #[doc = r" Returns a `Mut<>` with a smaller lifetime."]
    #[doc = r" This is useful if you have `&mut"]
    #[doc = stringify!(DefResMut)]
    #[doc = r" <T>`, but you need a `Mut<T>`."]
    pub fn reborrow(&mut self) -> Mut<'_, T> {
        Mut {
            value: self.value,
            ticks: ComponentTicksMut {
                added: self.ticks.added,
                changed: self.ticks.changed,
                changed_by: self.ticks.changed_by.as_deref_mut(),
                last_run: self.ticks.last_run,
                this_run: self.ticks.this_run,
            },
        }
    }
    #[doc = r" Maps to an inner value by applying a function to the contained reference, without flagging a change."]
    #[doc = r""]
    #[doc = r" You should never modify the argument passed to the closure -- if you want to modify the data"]
    #[doc = r" without flagging a change, consider using [`DetectChangesMut::bypass_change_detection`] to make your intent explicit."]
    #[doc = r""]
    #[doc = r" ```"]
    #[doc = r" # use bevy_ecs::prelude::*;"]
    #[doc = r" # #[derive(PartialEq)] pub struct Vec2;"]
    #[doc = r" # impl Vec2 { pub const ZERO: Self = Self; }"]
    #[doc = r" # #[derive(Component)] pub struct Transform { translation: Vec2 }"]
    #[doc = r" // When run, zeroes the translation of every entity."]
    #[doc = r" fn reset_positions(mut transforms: Query<&mut Transform>) {"]
    #[doc = r"     for transform in &mut transforms {"]
    #[doc = r"         // We pinky promise not to modify `t` within the closure."]
    #[doc = r"         // Breaking this promise will result in logic errors, but will never cause undefined behavior."]
    #[doc = r"         let mut translation = transform.map_unchanged(|t| &mut t.translation);"]
    #[doc = r"         // Only reset the translation if it isn't already zero;"]
    #[doc = r"         translation.set_if_neq(Vec2::ZERO);"]
    #[doc = r"     }"]
    #[doc = r" }"]
    #[doc = r" # bevy_ecs::system::assert_is_system(reset_positions);"]
    #[doc = r" ```"]
    pub fn map_unchanged<U: ?Sized>(self, f: impl FnOnce(&mut T) -> &mut U) -> Mut<'w, U> {
        Mut {
            value: f(self.value),
            ticks: self.ticks,
        }
    }
    #[doc = r" Optionally maps to an inner value by applying a function to the contained reference."]
    #[doc = r" This is useful in a situation where you need to convert a `Mut<T>` to a `Mut<U>`, but only if `T` contains `U`."]
    #[doc = r""]
    #[doc = r" As with `map_unchanged`, you should never modify the argument passed to the closure."]
    pub fn filter_map_unchanged<U: ?Sized>(
        self,
        f: impl FnOnce(&mut T) -> Option<&mut U>,
    ) -> Option<Mut<'w, U>> {
        let value = f(self.value);
        value.map(|value| Mut {
            value,
            ticks: self.ticks,
        })
    }
    #[doc = r" Optionally maps to an inner value by applying a function to the contained reference, returns an error on failure."]
    #[doc = r" This is useful in a situation where you need to convert a `Mut<T>` to a `Mut<U>`, but only if `T` contains `U`."]
    #[doc = r""]
    #[doc = r" As with `map_unchanged`, you should never modify the argument passed to the closure."]
    pub fn try_map_unchanged<U: ?Sized, E>(
        self,
        f: impl FnOnce(&mut T) -> Result<&mut U, E>,
    ) -> Result<Mut<'w, U>, E> {
        let value = f(self.value);
        value.map(|value| Mut {
            value,
            ticks: self.ticks,
        })
    }
    #[doc = r" Allows you access to the dereferenced value of this pointer without immediately"]
    #[doc = r" triggering change detection."]
    pub fn as_deref_mut(&mut self) -> Mut<'_, <T as Deref>::Target>
    where
        T: DerefMut,
    {
        self.reborrow().map_unchanged(|v| v.deref_mut())
    }
    */
}
impl<T: ?Sized + DefResource<Mutability = Mutable>, const N: usize> core::fmt::Debug
    for DefResMut<'_, T, N>
where
    T: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple(stringify!(ResMut))
            .field(&self.value)
            .finish()
    }
}

/*
impl<'w, T: Resource<Mutability = Mutable>> From<ResMut<'w, T>> for Mut<'w, T> {
    /// Convert this `ResMut` into a `Mut`. This allows keeping the change-detection feature of `Mut`
    /// while losing the specificity of `ResMut` for resources.
    fn from(other: ResMut<'w, T>) -> Mut<'w, T> {
        Mut {
            value: other.value,
            ticks: other.ticks,
        }
    }
}
*/

// SAFETY: Res ComponentId access is applied to SystemMeta. If this Res
// conflicts with any prior access, a panic will occur.
unsafe impl<'a, T: DefResource<Mutability = Mutable>, const N: usize> SystemParam
    for DefResMut<'a, T, N>
{
    type State = ComponentId;
    type Item<'w, 's> = DefResMut<'w, T, N>;

    fn init_state(world: &mut World) -> Self::State {
        DefineRegister::<T::Define>::arg_scope::<_, N>(world, |world, key, mut def| {
            def.resource::<T>(world, key.clone()).0
        })
    }

    fn init_access(
        &component_id: &Self::State,
        system_meta: &mut SystemMeta,
        component_access_set: &mut FilteredAccessSet,
        world: &mut World,
    ) {
        let mut filter = FilteredAccess::default();
        filter.add_write(component_id);
        filter.and_with(IS_RESOURCE);

        let conflicts = component_access_set.get_conflicts_single(&filter);
        if conflicts.is_empty() {
            component_access_set.add(filter);
            return;
        }

        /*
        let mut accesses = conflicts.format_conflict_list(world.as_unsafe_world_cell());
        // Access list may be empty (if access to all components requested)
        if !accesses.is_empty() {
            accesses.push(' ');
        }
        */
        panic!(
            "error[B0002]: ResMut<{}> in system {} conflicts with a previous system parameter. Consider removing the duplicate access or using `Without<IsResource>` to create disjoint Queries or merging conflicting Queries into a `ParamSet`. See: https://bevy.org/learn/errors/b0002",
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
        struct CastMutUntyped<'w> {
            pub(crate) value: PtrMut<'w>,
            pub(crate) ticks: ComponentTicksMut<'w>,
        }

        let value = world.get_resource_mut_by_id(component_id).ok_or_else(|| {
            SystemParamValidationError::invalid::<Self>("Resource does not exist")
        })?;

        // Safety: no
        let value = unsafe { core::mem::transmute::<MutUntyped<'_>, CastMutUntyped<'_>>(value) };

        Ok(DefResMut {
            value: value.value.deref_mut::<T>(),
            ticks: ComponentTicksMut {
                added: value.ticks.added,
                changed: value.ticks.changed,
                changed_by: value.ticks.changed_by,
                last_run: system_meta.get_last_run(),
                this_run: change_tick,
            },
        })
    }
}
