#![expect(clippy::undocumented_unsafe_blocks)]

use bevy_define::{
    Def, DefComponent, DefKey, DefRes, DefResource, Define, DefineRegister, EntityInsertDef as _,
    clone_def, get_resource, insert_resource,
};
use bevy_ecs::{component::Mutable, prelude::*, schedule::ScheduleLabel};

// Declare a new schedule label.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash, Default)]
struct Update;

type Key = std::borrow::Cow<'static, str>;

pub enum Variables {}
impl Define for Variables {
    type Key = Key;
    type Components = (A, B);
    type Resources = (VarRes,);
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct A(usize);

unsafe impl DefComponent for A {
    type Define = Variables;
    type Mutability = Mutable;
    const INDEX: usize = 0;

    fn clone_behavior() -> bevy_ecs::component::ComponentCloneBehavior {
        clone_def::<Self>()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct B(usize);

unsafe impl DefComponent for B {
    type Define = Variables;
    type Mutability = Mutable;
    const INDEX: usize = 1;
}

#[derive(Debug, PartialEq, Eq)]
pub struct VarRes(usize);
unsafe impl DefResource for VarRes {
    type Define = Variables;
    type Mutability = Mutable;
    const INDEX: usize = 0;
}

fn init() -> ([DefKey<Key>; 2], Entity, World) {
    let mut world = World::new();

    world.init_resource::<DefineRegister<Variables>>();

    let params = ["health", "stamina"];
    let [health, stamina]: [DefKey<Key>; _] = params.map(|key| DefKey(key.into()));

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

fn my_system(single: Single<'_, '_, (Def<A, 0>, Def<B, 1>)>, mut res: ResMut<'_, MustBeCalled>) {
    let item = single.into_inner();
    assert_eq!(item, (&A(42), &B(43)), "must be equal");
    res.0 = true;
}

#[test]
fn query() {
    let ([health, stamina], _entity, mut world) = init();

    {
        world.insert_resource(health.clone());
        let mut query = QueryBuilder::<(Def<A>, Def<B>)>::new(&mut world).build();
        let item = query.single(&world).unwrap();
        assert_eq!(item, (&A(42), &B(34)), "must be equal");
    }
    {
        world.insert_resource(stamina.clone());
        let mut query = QueryBuilder::<(Def<A>, Def<B>)>::new(&mut world).build();
        let item = query.single(&world).unwrap();
        assert_eq!(item, (&A(24), &B(43)), "must be equal");
    }

    {
        world.insert_resource(health.arg::<0>());
        world.insert_resource(stamina.arg::<1>());
        let mut query = QueryBuilder::<(Def<A, 0>, Def<B, 1>)>::new(&mut world).build();
        let item = query.single(&world).unwrap();
        assert_eq!(item, (&A(42), &B(43)), "must be equal");
    }
}

#[test]
fn resource() {
    let ([health, stamina], _entity, mut world) = init();

    insert_resource(&mut world, health.key(), VarRes(42));
    world.flush();

    let health_res = get_resource::<VarRes>(&mut world, health.key());
    assert_eq!(health_res, Some(&VarRes(42)));

    insert_resource(&mut world, stamina.key(), VarRes(34));
    world.flush();

    let stamina_res = get_resource::<VarRes>(&mut world, stamina.key());
    assert_eq!(stamina_res, Some(&VarRes(34)));
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

    world.resource_scope(|world, mut def: Mut<'_, DefineRegister<Variables>>| {
        let a_health = def.component::<A>(world, health.key());
        let a_stamina = def.component::<A>(world, stamina.key());

        let b_health = def.component::<B>(world, health.key());
        let b_stamina = def.component::<B>(world, stamina.key());

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
