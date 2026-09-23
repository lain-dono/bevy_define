#![expect(clippy::undocumented_unsafe_blocks)]

use bevy_define::{AnyDef, Def, DefComponent, DefKey, DefineRegister, EntityDef as _, clone_def};
use bevy_ecs::{component::Mutable, prelude::*, schedule::ScheduleLabel};
use std::str::FromStr;

// Declare a new schedule label.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash, Default)]
struct Update;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct A(usize);

unsafe impl DefComponent for A {
    type Mutability = Mutable;

    fn clone_behavior() -> bevy_ecs::component::ComponentCloneBehavior {
        clone_def::<Self>()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct B(usize);

unsafe impl DefComponent for B {
    type Mutability = Mutable;
}

fn init() -> ([DefKey; 2], Entity, World) {
    let mut world = World::new();

    world.init_resource::<DefineRegister>();

    let params = ["health", "stamina"];
    let [health, stamina]: [DefKey; _] = params.map(|key| DefKey::from_str(key).unwrap());

    let entity = world.spawn_empty().id();
    world
        .commands()
        .entity(entity)
        .insert_def(health.key(), A(42))
        .insert_def(health.key(), B(34))
        .insert_def(stamina.key(), A(24))
        .insert_def(stamina.key(), B(43));

    world.flush();

    ([health, stamina], entity, world)
}

#[derive(Resource, Default)]
struct MustBeCalled(bool);

fn my_system(single: Single<'_, '_, (Def<&A, 0>, Def<&B, 1>)>, mut res: ResMut<'_, MustBeCalled>) {
    let item = single.into_inner();
    assert_eq!(item, (&A(42), &B(43)), "must be equal");
    res.0 = true;
}

#[test]
fn query() {
    let ([health, stamina], _entity, mut world) = init();

    {
        world.insert_resource(health.clone());
        let mut query = QueryBuilder::<(Def<&A>, Def<&B>)>::new(&mut world).build();
        let item = query.single(&world).unwrap();
        assert_eq!(item, (&A(42), &B(34)), "must be equal");
    }
    {
        world.insert_resource(stamina.clone());
        let mut query = QueryBuilder::<(Def<&A>, Def<&B>)>::new(&mut world).build();
        let item = query.single(&world).unwrap();
        assert_eq!(item, (&A(24), &B(43)), "must be equal");
    }

    {
        world.insert_resource(health.arg::<0>());
        world.insert_resource(stamina.arg::<1>());
        let mut query = QueryBuilder::<(Def<&A, 0>, Def<&B, 1>)>::new(&mut world).build();
        let item = query.single(&world).unwrap();
        assert_eq!(item, (&A(42), &B(43)), "must be equal");
    }
}

#[test]
fn any_query() {
    let (_, _entity, mut world) = init();

    {
        let mut query = QueryBuilder::<AnyDef<&A>>::new(&mut world).build();
        let item = query.single(&world).unwrap();
        let variations = item.iter().collect::<Vec<Ref<_>>>();
        assert_eq!(&*variations[0], &A(42));
        assert_eq!(&*variations[1], &A(24));
    }

    {
        let mut query = QueryBuilder::<AnyDef<&B>>::new(&mut world).build();
        let item = query.single(&world).unwrap();
        let variations = item.iter().collect::<Vec<Ref<_>>>();
        assert_eq!(&*variations[0], &B(34));
        assert_eq!(&*variations[1], &B(43));
    }
}

#[test]
fn schedule_manual() {
    let ([health, stamina], _entity, mut world) = init();

    world.init_resource::<MustBeCalled>();

    let mut schedule = Schedule::default();
    schedule.add_systems(my_system);

    world.insert_resource(health.arg::<0>());
    world.insert_resource(stamina.arg::<1>());
    schedule.initialize(&mut world).unwrap();

    schedule.run(&mut world);

    assert!(world.resource::<MustBeCalled>().0);
}

#[test]

fn schedule_resource() {
    let ([health, stamina], _entity, mut world) = init();

    world.init_resource::<MustBeCalled>();

    let mut app = world.get_resource_or_init::<Schedules>();
    app.add_systems(Update, my_system);

    world.schedule_scope(Update, |world, schedule| {
        world.insert_resource(health.arg::<0>());
        world.insert_resource(stamina.arg::<1>());
        schedule.initialize(world).unwrap();
    });

    world.run_schedule(Update);

    assert!(world.resource::<MustBeCalled>().0);
}

#[test]
fn clone() {
    let ([health, stamina], entity, mut world) = init();

    world.resource_scope(|world, mut def: Mut<'_, DefineRegister>| {
        let (a_health, _) = def.component::<A>(world, health.key());
        let (a_stamina, _) = def.component::<A>(world, stamina.key());

        let (b_health, _) = def.component::<B>(world, health.key());
        let (b_stamina, _) = def.component::<B>(world, stamina.key());

        let entity_clone = world.spawn_empty().id();

        bevy_ecs::entity::EntityCloner::build_opt_out(world).clone_entity(entity, entity_clone);

        let entity = world.entity(entity_clone);
        let archetype = entity.archetype();

        assert!(archetype.components().contains(&a_health));
        assert!(archetype.components().contains(&a_stamina));

        assert!(!archetype.components().contains(&b_health));
        assert!(!archetype.components().contains(&b_stamina));
    });
}
