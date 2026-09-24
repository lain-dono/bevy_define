use super::{
    DefComponent, DefineRegister, Key,
    fetch::{AnyFetch, AnyQueryState, StorageSwitch},
};
use bevy_ecs::{
    archetype::Archetype,
    change_detection::{DetectChanges, Ref, Tick},
    component::{ComponentId, Components, Mutable, StorageType},
    entity::Entity,
    ptr::UnsafeCellDeref,
    query::{
        EcsAccessType, FilteredAccess, IterQueryData, QueryData, QueryItem, ReadOnlyQueryData,
        SingleEntityQueryData, WorldQuery,
    },
    storage::{SparseSets, Table, TableRow},
    world::{Mut, World, unsafe_world_cell::UnsafeWorldCell},
};
use std::{iter::Filter, marker::PhantomData};

pub struct AnyDef<T: ?Sized>(PhantomData<T>);

pub struct AnyRead<'a, T: DefComponent> {
    pub(crate) registry: &'a DefineRegister,
    pub(crate) data: StorageSwitch<
        T,
        // Read-only access to the global trait registry.
        // Since no one outside of the crate can name the registry type,
        // we can be confident that no write accesses will conflict with this.
        (TableRow, &'a Table),
        // This grants shared access to all sparse set components,
        // but in practice we will only read the components specified in `self.registry`.
        // The fetch impl registers read-access for all of these components,
        // so there will be no runtime conflicts.
        (Entity, &'a SparseSets),
    >,
    pub(crate) last_run: Tick,
    pub(crate) this_run: Tick,
}

impl<'w, T: DefComponent> IntoIterator for AnyRead<'w, T> {
    type Item = (&'w [u8], Ref<'w, T>);
    type IntoIter = ReadIter<'w, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        ReadIter {
            components: self.registry.types::<T>().iter(),
            data: self.data,
            last_run: self.last_run,
            this_run: self.this_run,
        }
    }
}

impl<'w, T: DefComponent> IntoIterator for &AnyRead<'w, T> {
    type Item = (&'w [u8], Ref<'w, T>);
    type IntoIter = ReadIter<'w, T>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        ReadIter {
            components: self.registry.types::<T>().iter(),
            data: self.data,
            last_run: self.last_run,
            this_run: self.this_run,
        }
    }
}

impl<'w, T: DefComponent> AnyRead<'w, T> {
    pub fn iter(&self) -> ReadIter<'w, T> {
        self.into_iter()
    }
}

pub struct ReadIter<'a, T: DefComponent> {
    pub(crate) components: core::slice::Iter<'a, (Key, ComponentId)>,
    pub(crate) data: StorageSwitch<T, (TableRow, &'a Table), (Entity, &'a SparseSets)>,
    pub(crate) last_run: Tick,
    pub(crate) this_run: Tick,
}

impl<'a, T: DefComponent> ReadIter<'a, T> {
    pub fn intersect<I, V>(self, b: I) -> IntersectionIter<'a, Self, I, Ref<'a, T>, V>
    where
        I: Iterator<Item = (&'a [u8], V)>,
    {
        IntersectionIter(self.peekable(), b.peekable())
    }

    pub fn added(self) -> Filter<Self, fn(&(&'a [u8], Ref<'a, T>)) -> bool> {
        self.filter(|(_, value)| value.is_added())
    }

    pub fn changed(self) -> Filter<Self, fn(&(&'a [u8], Ref<'a, T>)) -> bool> {
        self.filter(|(_, value)| value.is_changed())
    }
}

impl<'a, T: DefComponent> Iterator for ReadIter<'a, T> {
    type Item = (&'a [u8], Ref<'a, T>);

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.components.len()))
    }

    fn next(&mut self) -> Option<Self::Item> {
        let (key, value, added, changed, location) = match T::STORAGE_TYPE {
            StorageType::Table => {
                // SAFETY: C::STORAGE_TYPE == StorageType::Table
                let (row, table) = unsafe { self.data.table };

                // Iterate the remaining table components that are registered,
                // until we find one that exists in the table.
                // SAFETY: we know that the `row` is a valid index.
                let (key, value, component_id, location) =
                    self.components.find_map(|&(ref key, component)| unsafe {
                        let ptr = table.get_component(component, row)?;
                        let location = table.get_changed_by(component, row);
                        Some((key.as_ref(), ptr.deref::<T>(), component, location))
                    })?;

                let [added_tick, changed_tick] = [
                    table.get_added_tick(component_id, row)?,
                    table.get_changed_tick(component_id, row)?,
                ];

                (key, value, added_tick, changed_tick, location.transpose()?)
            }
            StorageType::SparseSet => {
                // SAFETY: C::STORAGE_TYPE == StorageType::SparseSet
                let (entity, sets) = unsafe { self.data.sparse_set };

                let (key, value, ticks, location) =
                    self.components.find_map(|&(ref key, component)| unsafe {
                        let set = sets.get(component)?;
                        let (ptr, ticks) = set.get_with_ticks(entity)?;
                        Some((key.as_ref(), ptr.deref::<T>(), ticks, ticks.changed_by))
                    })?;
                (key, value, ticks.added, ticks.changed, location)
            }
        };

        // SAFETY:
        // Read access has been registered, so we can dereference it immutably.
        let value = Ref::new(
            value,
            unsafe { added.deref() },
            unsafe { changed.deref() },
            self.last_run,
            self.this_run,
            unsafe { location.map(|loc| loc.deref()) },
        );
        Some((key, value))
    }
}

unsafe impl<T: DefComponent> IterQueryData for AnyDef<&T> {}
unsafe impl<T: DefComponent> SingleEntityQueryData for AnyDef<&T> {}
unsafe impl<T: DefComponent> QueryData for AnyDef<&T> {
    type ReadOnly = Self;

    const IS_READ_ONLY: bool = true;
    const IS_ARCHETYPAL: bool = false;

    type Item<'w, 's> = AnyRead<'w, T>;

    #[inline]
    fn shrink<'wlong: 'wshort, 'wshort, 's>(
        item: QueryItem<'wlong, 's, Self>,
    ) -> QueryItem<'wshort, 's, Self> {
        item
    }

    #[inline]
    unsafe fn fetch<'w, 's>(
        _state: &'s Self::State,
        fetch: &mut Self::Fetch<'w>,
        entity: Entity,
        row: TableRow,
    ) -> Option<Self::Item<'w, 's>> {
        Some(fetch.read(entity, row))
    }

    fn iter_access(_state: &Self::State) -> impl Iterator<Item = EcsAccessType<'_>> {
        core::iter::empty()
    }
}
unsafe impl<T: DefComponent> ReadOnlyQueryData for AnyDef<&T> {}

// SAFETY: We only access the components registered in the trait registry.
// This is known to match the set of components in the TraitQueryState,
// which is used to match archetypes and register world access.
unsafe impl<T: DefComponent> WorldQuery for AnyDef<&T> {
    type Fetch<'w> = AnyFetch<'w, T>;
    type State = AnyQueryState<T>;

    #[inline]
    unsafe fn init_fetch<'w>(
        world: UnsafeWorldCell<'w>,
        _state: &Self::State,
        last_run: Tick,
        this_run: Tick,
    ) -> Self::Fetch<'w> {
        unsafe { AnyFetch::init(world, last_run, this_run) }
    }

    const IS_DENSE: bool = false;

    #[inline]
    unsafe fn set_archetype<'w>(
        fetch: &mut Self::Fetch<'w>,
        _state: &Self::State,
        _archetype: &'w Archetype,
        table: &'w Table,
    ) {
        fetch.set_table(table);
    }

    unsafe fn set_table<'w>(fetch: &mut Self::Fetch<'w>, _state: &Self::State, table: &'w Table) {
        fetch.set_table(table);
    }

    #[inline]
    fn update_component_access(state: &Self::State, access: &mut FilteredAccess) {
        state.update::<false>(access);
    }

    #[inline]
    fn init_state(world: &mut World) -> Self::State {
        Self::State::init(world)
    }

    #[inline]
    fn get_state(_: &Components) -> Option<Self::State> {
        None
    }

    #[inline]
    fn matches_component_set(state: &Self::State, f: &impl Fn(ComponentId) -> bool) -> bool {
        state.matches(f)
    }

    #[inline]
    fn shrink_fetch<'wlong: 'wshort, 'wshort>(fetch: Self::Fetch<'wlong>) -> Self::Fetch<'wshort> {
        fetch
    }
}

pub struct AnyWrite<'a, T: DefComponent> {
    // Read-only access to the global trait registry.
    // Since no one outside of the crate can name the registry type,
    // we can be confident that no write accesses will conflict with this.
    pub(crate) registry: &'a DefineRegister,

    pub(crate) data: StorageSwitch<
        T,
        (TableRow, &'a Table),
        // This grants shared mutable access to all sparse set components,
        // but in practice we will only modify the components specified in `self.registry`.
        // The fetch impl registers write-access for all of these components,
        // guaranteeing us exclusive access at runtime.
        (Entity, &'a SparseSets),
    >,

    pub(crate) last_run: Tick,
    pub(crate) this_run: Tick,
}

impl<T: DefComponent> AnyWrite<'_, T> {
    pub fn iter(&self) -> ReadIter<'_, T> {
        self.into_iter()
    }

    pub fn iter_mut(&mut self) -> WriteIter<'_, T> {
        self.into_iter()
    }
}

impl<'w, T: DefComponent> IntoIterator for AnyWrite<'w, T> {
    type Item = (&'w [u8], Mut<'w, T>);
    type IntoIter = WriteIter<'w, T>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        WriteIter {
            components: self.registry.types::<T>().iter(),
            data: self.data,
            last_run: self.last_run,
            this_run: self.this_run,
        }
    }
}

impl<'a, T: DefComponent> IntoIterator for &'a AnyWrite<'_, T> {
    type Item = (&'a [u8], Ref<'a, T>);
    type IntoIter = ReadIter<'a, T>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        ReadIter {
            components: self.registry.types::<T>().iter(),
            data: self.data,
            last_run: self.last_run,
            this_run: self.this_run,
        }
    }
}

impl<'a, T: DefComponent> IntoIterator for &'a mut AnyWrite<'_, T> {
    type Item = (&'a [u8], Mut<'a, T>);
    type IntoIter = WriteIter<'a, T>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        WriteIter {
            components: self.registry.types::<T>().iter(),
            data: self.data,
            last_run: self.last_run,
            this_run: self.this_run,
        }
    }
}

pub struct WriteIter<'a, T: DefComponent> {
    // SAFETY: These two iterators must have equal length.
    pub(crate) components: core::slice::Iter<'a, (Key, ComponentId)>,
    /// SAFETY: Given the same trait type and same archetype,
    /// no two instances of this struct may have the same `row`.
    pub(crate) data: StorageSwitch<T, (TableRow, &'a Table), (Entity, &'a SparseSets)>,
    pub(crate) last_run: Tick,
    pub(crate) this_run: Tick,
}

impl<'a, T: DefComponent> WriteIter<'a, T> {
    pub fn intersect<I, V>(self, b: I) -> IntersectionIter<'a, Self, I, Mut<'a, T>, V>
    where
        I: Iterator<Item = (&'a [u8], V)>,
    {
        IntersectionIter(self.peekable(), b.peekable())
    }

    pub fn added(self) -> Filter<Self, fn(&(&'a [u8], Mut<'a, T>)) -> bool> {
        self.filter(|(_, value)| value.is_added())
    }

    pub fn changed(self) -> Filter<Self, fn(&(&'a [u8], Mut<'a, T>)) -> bool> {
        self.filter(|(_, value)| value.is_changed())
    }
}

impl<'a, T: DefComponent> Iterator for WriteIter<'a, T> {
    type Item = (&'a [u8], Mut<'a, T>);

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.components.len()))
    }

    fn next(&mut self) -> Option<Self::Item> {
        let (key, value, added, changed, location) = match T::STORAGE_TYPE {
            StorageType::Table => {
                // SAFETY: C::STORAGE_TYPE == StorageType::Table
                let (row, table) = unsafe { self.data.table };

                // Iterate the remaining table components that are registered,
                // until we find one that exists in the table.
                // SAFETY: we know that the `row` is a valid index.
                let (key, value, component_id, location) =
                    self.components.find_map(|&(ref key, component)| unsafe {
                        let ptr = table.get_component(component, row)?.assert_unique();
                        let location = table.get_changed_by(component, row);
                        Some((key.as_ref(), ptr.deref_mut::<T>(), component, location))
                    })?;

                let [added_tick, changed_tick] = [
                    table.get_added_tick(component_id, row)?,
                    table.get_changed_tick(component_id, row)?,
                ];

                (key, value, added_tick, changed_tick, location.transpose()?)
            }
            StorageType::SparseSet => {
                // SAFETY: C::STORAGE_TYPE == StorageType::SparseSet
                let (entity, sets) = unsafe { self.data.sparse_set };

                let (key, value, ticks, location) =
                    self.components.find_map(|&(ref key, component)| unsafe {
                        let set = sets.get(component)?;
                        let (ptr, ticks) = set.get_with_ticks(entity)?;
                        let ptr = ptr.assert_unique().deref_mut::<T>();
                        Some((key.as_ref(), ptr, ticks, ticks.changed_by))
                    })?;
                (key, value, ticks.added, ticks.changed, location)
            }
        };

        let value = Mut::new(
            value,
            unsafe { added.deref_mut() },
            unsafe { changed.deref_mut() },
            self.last_run,
            self.this_run,
            unsafe { location.map(|loc| loc.deref_mut()) },
        );

        Some((key, value))
    }
}

unsafe impl<T: DefComponent<Mutability = Mutable>> IterQueryData for AnyDef<&mut T> {}
unsafe impl<T: DefComponent<Mutability = Mutable>> SingleEntityQueryData for AnyDef<&mut T> {}
unsafe impl<'a, T: DefComponent<Mutability = Mutable>> QueryData for AnyDef<&'a mut T> {
    type ReadOnly = AnyDef<&'a T>;

    const IS_READ_ONLY: bool = false;
    const IS_ARCHETYPAL: bool = false;

    type Item<'w, 's> = AnyWrite<'w, T>;

    #[inline]
    fn shrink<'wlong: 'wshort, 'wshort, 's>(
        item: QueryItem<'wlong, 's, Self>,
    ) -> QueryItem<'wshort, 's, Self> {
        item
    }

    #[inline]
    unsafe fn fetch<'w, 's>(
        _state: &'s Self::State,
        fetch: &mut Self::Fetch<'w>,
        entity: Entity,
        row: TableRow,
    ) -> Option<Self::Item<'w, 's>> {
        Some(fetch.write(entity, row))
    }

    fn iter_access(_state: &Self::State) -> impl Iterator<Item = EcsAccessType<'_>> {
        core::iter::empty()
    }
}

// SAFETY: We only access the components registered in the trait registry.
// This is known to match the set of components in the TraitQueryState,
// which is used to match archetypes and register world access.
unsafe impl<T: DefComponent<Mutability = Mutable>> WorldQuery for AnyDef<&mut T> {
    type Fetch<'w> = AnyFetch<'w, T>;
    type State = AnyQueryState<T>;

    #[inline]
    unsafe fn init_fetch<'w>(
        world: UnsafeWorldCell<'w>,
        _state: &Self::State,
        last_run: Tick,
        this_run: Tick,
    ) -> Self::Fetch<'w> {
        unsafe { AnyFetch::init(world, last_run, this_run) }
    }

    const IS_DENSE: bool = false;

    #[inline]
    unsafe fn set_archetype<'w>(
        fetch: &mut Self::Fetch<'w>,
        _state: &Self::State,
        _archetype: &'w Archetype,
        table: &'w Table,
    ) {
        fetch.set_table(table);
    }

    #[inline]
    unsafe fn set_table<'w>(fetch: &mut Self::Fetch<'w>, _state: &Self::State, table: &'w Table) {
        fetch.set_table(table);
    }

    #[inline]
    fn update_component_access(state: &Self::State, access: &mut FilteredAccess) {
        state.update::<false>(access);
    }

    #[inline]
    fn init_state(world: &mut World) -> Self::State {
        Self::State::init(world)
    }

    #[inline]
    fn get_state(_: &Components) -> Option<Self::State> {
        None
    }

    #[inline]
    fn matches_component_set(state: &Self::State, f: &impl Fn(ComponentId) -> bool) -> bool {
        state.matches(f)
    }

    #[inline]
    fn shrink_fetch<'wlong: 'wshort, 'wshort>(fetch: Self::Fetch<'wlong>) -> Self::Fetch<'wshort> {
        fetch
    }
}

pub struct IntersectionIter<'a, A, B, AI, BI>(std::iter::Peekable<A>, std::iter::Peekable<B>)
where
    A: Iterator<Item = (&'a [u8], AI)>,
    B: Iterator<Item = (&'a [u8], BI)>;

impl<'a, A, B, AI, BI> Iterator for IntersectionIter<'a, A, B, AI, BI>
where
    A: Iterator<Item = (&'a [u8], AI)>,
    B: Iterator<Item = (&'a [u8], BI)>,
{
    type Item = (&'a [u8], AI, BI);

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (a, b) = (self.0.size_hint(), self.1.size_hint());
        let max = a.1.zip(b.1).map(|(a, b)| usize::max(a, b));
        (usize::min(a.0, b.0), max)
    }

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let &(a, _) = self.0.peek()?;
            let &(b, _) = self.1.peek()?;

            match a.cmp(b) {
                std::cmp::Ordering::Less => {
                    self.0.next()?;
                }
                std::cmp::Ordering::Greater => {
                    self.1.next()?;
                }
                std::cmp::Ordering::Equal => {
                    let (key, a) = self.0.next().unwrap();
                    let (_, b) = self.1.next().unwrap();
                    return Some((key, a, b));
                }
            };
        }
    }
}
