//! The levels, and loading them.
//!
//! Each level is a list of boxes (see `map.rs`), a place to start, posts for the
//! hunters, a mood (sky, sun, lamps), and an exit. Kill every hunter and the
//! exit's beacon lights up; reach it to clear the level. Training is the old
//! test map: hunters come back, practice dummies, and no exit.
//!
//! Loading a level throws away everything belonging to the last one (anything
//! with [`LevelThing`]) and builds the new one.

use std::f32::consts::PI;

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::character::CharacterKit;
use crate::enemies::{self, Enemy, EnemyKilled};
use crate::heart::Run;
use crate::map::{self, Kind, MapCollision, Piece, block};
use crate::player::{MovementTick, PlayerStatus, RespawnPlayer};
use crate::retro::VIEW_MODEL_LAYER;
use crate::targets;

pub struct LevelsPlugin;

impl Plugin for LevelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurrentLevel>()
            .init_resource::<LevelStats>()
            .add_message::<LoadLevel>()
            .add_message::<LevelCleared>()
            .add_systems(Startup, (make_exit_look, spawn_hud))
            .add_systems(Update, load_level.in_set(LevelLoad))
            .add_systems(FixedUpdate, watch_objective.after(MovementTick))
            .add_systems(Update, (light_exit, update_hud));
    }
}

/// Which level is training (the rest are the campaign, in order).
pub const TRAINING: usize = 0;
/// How many levels there are, training included.
pub const COUNT: usize = 4;

/// The level's name and a line about it, for the menus and the HUD.
pub fn info(index: usize) -> (&'static str, &'static str) {
    match index {
        TRAINING => (
            "TRAINING",
            "The old test yard. Hunters get back up, dummies to shoot, no exit.",
        ),
        1 => (
            "THE YARD",
            "A rail yard at dusk. Stacks of containers, a loading shed, and the gate out.",
        ),
        2 => (
            "THE ROOFTOPS",
            "A city block at night. The way out is the top of the clock tower.",
        ),
        _ => (
            "THE FOUNDRY",
            "Catwalks around a furnace pit. Clear the floor and climb down into the heat.",
        ),
    }
}

/// Belongs to the loaded level: thrown away when another one loads.
#[derive(Component)]
pub struct LevelThing;

/// Load level `0` (an index, see [`info`]): build it, and put the player at its
/// start.
#[derive(Message, Clone, Copy)]
pub struct LoadLevel(pub usize);

/// The player reached the open exit.
#[derive(Message, Clone, Copy)]
pub struct LevelCleared;

/// Where levels are loaded. Order systems that ask for a load before it, so
/// it happens the same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LevelLoad;

/// The level that's loaded, and how it's going.
#[derive(Resource, Default)]
pub struct CurrentLevel {
    pub index: usize,
    /// Where the player starts (feet), and which way they face.
    pub spawn: Vec3,
    pub facing: f32,
    /// Where the exit is (the middle of its pad), if the level has one.
    pub exit: Option<Vec3>,
    pub hunters: usize,
    pub hunters_left: usize,
    pub cleared: bool,
}

/// Time on the level and kills, for the level-clear screen.
#[derive(Resource, Default)]
pub struct LevelStats {
    pub time: f32,
    pub kills: u32,
}

/// The light and colour of a level.
struct Mood {
    sky: Color,
    sun: Color,
    sun_strength: f32,
    /// Which way the sunlight travels.
    sun_direction: Vec3,
    ambient: Color,
    ambient_strength: f32,
    /// Multiplies every surface colour.
    tint: Color,
    lamp: Color,
}

pub struct Level {
    pieces: Vec<Piece>,
    pub spawn: Vec3,
    pub facing: f32,
    pub hunters: Vec<Vec3>,
    dummies: Vec<Vec3>,
    pub exit: Option<Vec3>,
    /// Lamps light up the area around them.
    lamps: Vec<Vec3>,
    mood: Mood,
}

impl Level {
    pub fn collision(&self) -> pumpdump_movement::StaticWorld {
        map::collision(&self.pieces)
    }
}

pub fn level(index: usize) -> Level {
    match index {
        TRAINING => training(),
        1 => yard(),
        2 => rooftops(),
        _ => foundry(),
    }
}

// ---- building helpers ----

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// A box between two opposite corners, given in any order.
fn put(p: &mut Vec<Piece>, a: Vec3, b: Vec3, kind: Kind) {
    p.push(block(a.min(b), a.max(b), kind));
}

/// The ground: top at y = 0, a little bigger than the walls around it.
fn ground(p: &mut Vec<Piece>, x0: f32, z0: f32, x1: f32, z1: f32) {
    put(
        p,
        v(x0 - 200.0, -16.0, z0 - 200.0),
        v(x1 + 200.0, 0.0, z1 + 200.0),
        Kind::Floor,
    );
}

/// Walls all the way round, `height` tall, just outside the given area.
fn perimeter(p: &mut Vec<Piece>, x0: f32, z0: f32, x1: f32, z1: f32, height: f32) {
    let (lo, hi) = (
        v(x0.min(x1), 0.0, z0.min(z1)),
        v(x0.max(x1), height, z0.max(z1)),
    );
    const T: f32 = 32.0;
    put(
        p,
        v(lo.x - T, 0.0, lo.z - T),
        v(lo.x, height, hi.z + T),
        Kind::Wall,
    );
    put(
        p,
        v(hi.x, 0.0, lo.z - T),
        v(hi.x + T, height, hi.z + T),
        Kind::Wall,
    );
    put(p, v(lo.x, 0.0, lo.z - T), v(hi.x, height, lo.z), Kind::Wall);
    put(p, v(lo.x, 0.0, hi.z), v(hi.x, height, hi.z + T), Kind::Wall);
}

/// A solid brick building with a concrete roof (whose top is 10 above `height`).
fn building(p: &mut Vec<Piece>, x0: f32, z0: f32, x1: f32, z1: f32, height: f32) {
    put(p, v(x0, 0.0, z0), v(x1, height, z1), Kind::Wall);
    let (lo, hi) = (x0.min(x1), x0.max(x1));
    let (near, far) = (z0.max(z1), z0.min(z1));
    put(
        p,
        v(lo - 6.0, height, far - 6.0),
        v(hi + 6.0, height + 10.0, near + 6.0),
        Kind::Roof,
    );
}

/// A wooden crate `size` across with its bottom at `y`.
fn crate_at(p: &mut Vec<Piece>, x: f32, y: f32, z: f32, size: f32) {
    let h = size * 0.5;
    put(
        p,
        v(x - h, y, z - h),
        v(x + h, y + size, z + h),
        Kind::Crate,
    );
}

/// A stack of `count` shipping containers (each 96 x 102 x 480), long side
/// along Z (or X).
fn containers(p: &mut Vec<Piece>, x: f32, z: f32, count: u32, along_z: bool, paint: u8) {
    let half = if along_z {
        v(48.0, 51.0, 240.0)
    } else {
        v(240.0, 51.0, 48.0)
    };
    for k in 0..count {
        let y = 102.0 * k as f32;
        put(
            p,
            v(x - half.x, y, z - half.z),
            v(x + half.x, y + 102.0, z + half.z),
            Kind::Container(paint + k as u8),
        );
    }
}

/// A parked car, nose along Z: cover for one.
fn car(p: &mut Vec<Piece>, x: f32, z: f32, paint: u8) {
    put(
        p,
        v(x - 45.0, 0.0, z - 95.0),
        v(x + 45.0, 52.0, z + 95.0),
        Kind::Container(paint),
    );
    put(
        p,
        v(x - 40.0, 52.0, z - 50.0),
        v(x + 40.0, 88.0, z + 40.0),
        Kind::Container(paint),
    );
}

/// A lamp post `height` tall, and its light.
fn lamp(p: &mut Vec<Piece>, lamps: &mut Vec<Vec3>, x: f32, z: f32, height: f32) {
    put(
        p,
        v(x - 4.0, 0.0, z - 4.0),
        v(x + 4.0, height, z + 4.0),
        Kind::Iron,
    );
    put(
        p,
        v(x - 14.0, height, z - 14.0),
        v(x + 14.0, height + 8.0, z + 14.0),
        Kind::Marker,
    );
    lamps.push(v(x, height - 12.0, z));
}

/// Steps 16 high and 32 deep up to a ledge `height` tall whose edge is at
/// `edge_z`, between `x0` and `x1`. `side` is +1 for steps on the +Z side of
/// the edge, -1 for the -Z side.
fn steps_up(p: &mut Vec<Piece>, x0: f32, x1: f32, edge_z: f32, height: f32, side: f32) {
    const RISE: f32 = 16.0;
    const DEPTH: f32 = 32.0;
    let mut k = 1;
    while height - RISE * k as f32 > 0.0 {
        let top = height - RISE * k as f32;
        let z = edge_z + side * DEPTH * (k - 1) as f32;
        put(p, v(x0, 0.0, z), v(x1, top, z + side * DEPTH), Kind::Step);
        k += 1;
    }
}

// ---- the levels ----

/// The old greybox test map: ramps and stairs of every steepness, ledges,
/// corridors for wall-running and floating platforms for the hook.
fn training() -> Level {
    let mut p = Vec::new();

    // Floor, top at y = 0.
    p.push(block(
        v(-6000.0, -16.0, -6000.0),
        v(6000.0, 0.0, 3000.0),
        Kind::Floor,
    ));

    // Ramps on the right: walkable 15°, 30°, 40°; too steep 55°. Max slope is in movement.ron.
    for (i, degrees) in [15.0, 30.0, 40.0, 55.0].into_iter().enumerate() {
        map::ramp(
            &mut p,
            450.0 + 200.0 * i as f32,
            -300.0,
            400.0,
            degrees,
            160.0,
        );
    }

    // Stairs on the left: 8, 12 and 16 unit steps (all under the 18-unit step height).
    for (i, rise) in [8.0, 12.0, 16.0].into_iter().enumerate() {
        map::stairs(&mut p, -450.0 - 200.0 * i as f32, -300.0, rise, 8, 160.0);
    }

    // Behind the spawn: single ledges from walk-up-able to jump-only.
    for (i, height) in [12.0, 18.0, 20.0, 24.0, 32.0, 48.0, 64.0]
        .into_iter()
        .enumerate()
    {
        let x = -540.0 + 160.0 * i as f32;
        p.push(block(
            v(x - 48.0, 0.0, 300.0),
            v(x + 48.0, height, 400.0),
            Kind::Step,
        ));
    }

    // Crates straight ahead.
    for (x, z, size) in [
        (-150.0, -650.0, 48.0),
        (60.0, -700.0, 64.0),
        (-40.0, -900.0, 96.0),
        (180.0, -1000.0, 128.0),
    ] {
        crate_at(&mut p, x, 0.0, z, size);
    }

    // Speed lane: a marker post every 100 units along x = 260, from the spawn line.
    for k in 0..14 {
        let z = -100.0 * k as f32;
        p.push(block(
            v(258.0, 0.0, z - 2.0),
            v(262.0, 40.0, z + 2.0),
            Kind::Marker,
        ));
    }

    // Long walls for wall-running: a corridor 300 wide, a lone wall, and a
    // staggered pair for chaining runs.
    let wall = |x: f32, z_from: f32, z_to: f32, height: f32| {
        block(
            v(x - 8.0, 0.0, z_to),
            v(x + 8.0, height, z_from),
            Kind::Wall,
        )
    };
    p.push(wall(-158.0, -1400.0, -5400.0, 320.0));
    p.push(wall(158.0, -1400.0, -5400.0, 320.0));
    p.push(wall(700.0, -1400.0, -5400.0, 400.0));
    p.push(wall(-900.0, -1400.0, -2000.0, 320.0));
    p.push(wall(-1300.0, -2100.0, -2700.0, 320.0));

    // Floating platforms to grapple up to, from low to high, and one that hangs
    // over the end of the corridor.
    for (x, y, z, size) in [
        (-600.0, 550.0, -800.0, 240.0),
        (500.0, 800.0, -1500.0, 200.0),
        (-1200.0, 1000.0, -1700.0, 260.0),
        (0.0, 1100.0, -3400.0, 320.0),
        (1300.0, 650.0, -2400.0, 240.0),
        (-400.0, 1400.0, -4600.0, 300.0),
    ] {
        let h = size * 0.5;
        p.push(block(
            v(x - h, y, z - h),
            v(x + h, y + 40.0, z + h),
            Kind::Platform,
        ));
    }

    Level {
        pieces: p,
        spawn: v(0.0, 1.0, 0.0),
        facing: 0.0,
        hunters: vec![
            v(-600.0, 0.0, -1600.0),
            v(420.0, 0.0, -1900.0),
            v(0.0, 0.0, -3200.0),
            v(-1100.0, 0.0, -2400.0),
            v(1500.0, 0.0, -800.0),
            v(-1500.0, 0.0, -600.0),
            v(900.0, 0.0, 700.0),
        ],
        dummies: vec![
            v(-250.0, 0.0, -500.0),
            // On the platform at the top of the 30° ramp.
            v(650.0, 230.94, -820.0),
            // Behind the spawn.
            v(-300.0, 0.0, 650.0),
        ],
        exit: None,
        lamps: Vec::new(),
        mood: Mood {
            sky: map::SKY,
            sun: Color::srgb(1.0, 0.82, 0.62),
            sun_strength: 4200.0,
            sun_direction: v(-0.4, -1.0, -0.6),
            ambient: Color::srgb(0.95, 0.78, 0.65),
            ambient_strength: 650.0,
            tint: Color::WHITE,
            lamp: Color::WHITE,
        },
    }
}

/// A rail yard at dusk: a fenced lot, lanes between stacked containers, a
/// loading shed and dock, and the tracks at the far end, where the exit is.
fn yard() -> Level {
    let mut p = Vec::new();
    let mut lamps = Vec::new();
    let (x0, z0, x1, z1) = (-1800.0, 1200.0, 1800.0, -7200.0);
    ground(&mut p, x0, z1, x1, z0);
    perimeter(&mut p, x0, z0, x1, z1, 500.0);

    // The lot you start in: a fence across with a gate in the middle.
    put(
        &mut p,
        v(-1800.0, 0.0, -420.0),
        v(-220.0, 240.0, -380.0),
        Kind::Wall,
    );
    put(
        &mut p,
        v(220.0, 0.0, -420.0),
        v(1800.0, 240.0, -380.0),
        Kind::Wall,
    );
    put(
        &mut p,
        v(-260.0, 0.0, -440.0),
        v(-200.0, 300.0, -360.0),
        Kind::Step,
    );
    put(
        &mut p,
        v(200.0, 0.0, -440.0),
        v(260.0, 300.0, -360.0),
        Kind::Step,
    );
    crate_at(&mut p, -900.0, 0.0, 400.0, 64.0);
    crate_at(&mut p, -800.0, 0.0, 300.0, 48.0);
    crate_at(&mut p, 1000.0, 0.0, 200.0, 96.0);
    crate_at(&mut p, 1000.0, 96.0, 200.0, 48.0);
    lamp(&mut p, &mut lamps, -300.0, -300.0, 260.0);
    lamp(&mut p, &mut lamps, 300.0, -300.0, 260.0);

    // Containers: four rows, stacked one to three high, with lanes between.
    for (x, stacks) in [
        (
            -1250.0,
            [(-900.0, 2), (-1500.0, 1), (-2100.0, 3), (-2700.0, 2)].as_slice(),
        ),
        (
            -650.0,
            [(-1100.0, 1), (-1900.0, 2), (-2900.0, 1)].as_slice(),
        ),
        (
            650.0,
            [(-800.0, 2), (-1600.0, 1), (-2400.0, 2), (-3000.0, 3)].as_slice(),
        ),
        (
            1250.0,
            [(-1200.0, 3), (-2000.0, 1), (-2800.0, 2)].as_slice(),
        ),
    ] {
        for (i, &(z, count)) in stacks.iter().enumerate() {
            containers(
                &mut p,
                x,
                z,
                count,
                true,
                (i as u8 + (x as i32 / 300).unsigned_abs() as u8) % 4,
            );
        }
    }
    // Low crates for cover down the middle lane.
    for (x, z, size) in [
        (0.0, -1300.0, 48.0),
        (-120.0, -1360.0, 48.0),
        (150.0, -2000.0, 64.0),
        (-100.0, -2700.0, 48.0),
        (80.0, -3100.0, 64.0),
    ] {
        crate_at(&mut p, x, 0.0, z, size);
    }

    // The loading shed: a roof on iron pillars, crates underneath.
    put(
        &mut p,
        v(-900.0, 420.0, -5000.0),
        v(900.0, 444.0, -3800.0),
        Kind::Roof,
    );
    for x in [-870.0, -290.0, 290.0, 870.0] {
        for z in [-3830.0, -4970.0] {
            put(
                &mut p,
                v(x - 20.0, 0.0, z - 20.0),
                v(x + 20.0, 420.0, z + 20.0),
                Kind::Iron,
            );
        }
    }
    crate_at(&mut p, -500.0, 0.0, -4100.0, 96.0);
    crate_at(&mut p, -500.0, 96.0, -4100.0, 48.0);
    crate_at(&mut p, -380.0, 0.0, -4130.0, 64.0);
    crate_at(&mut p, 450.0, 0.0, -4500.0, 96.0);
    crate_at(&mut p, 560.0, 0.0, -4560.0, 48.0);
    crate_at(&mut p, -200.0, 0.0, -4700.0, 64.0);
    // A ramp up beside the shed, to jump across onto its roof.
    map::ramp(&mut p, -1200.0, -3700.0, 520.0, 30.0, 160.0);
    lamp(&mut p, &mut lamps, -600.0, -3700.0, 300.0);
    lamp(&mut p, &mut lamps, 600.0, -3700.0, 300.0);

    // The loading dock along the east side, with steps at its south end.
    put(
        &mut p,
        v(900.0, 0.0, -5200.0),
        v(1700.0, 64.0, -3600.0),
        Kind::Step,
    );
    steps_up(&mut p, 1000.0, 1600.0, -3600.0, 64.0, 1.0);
    crate_at(&mut p, 1500.0, 64.0, -4400.0, 96.0);
    crate_at(&mut p, 1200.0, 64.0, -4900.0, 64.0);

    // Low walls to hide behind, then the tracks and two boxcars.
    for (a, b) in [(-1500.0, -700.0), (-300.0, 300.0), (700.0, 1500.0)] {
        put(&mut p, v(a, 0.0, -5640.0), v(b, 120.0, -5600.0), Kind::Wall);
    }
    for z in [-6100.0, -6200.0, -6500.0, -6600.0] {
        put(
            &mut p,
            v(-1800.0, 0.0, z - 6.0),
            v(1800.0, 8.0, z + 6.0),
            Kind::Iron,
        );
    }
    put(
        &mut p,
        v(-1500.0, 0.0, -6230.0),
        v(-500.0, 150.0, -6070.0),
        Kind::Container(0),
    );
    put(
        &mut p,
        v(500.0, 0.0, -6630.0),
        v(1500.0, 150.0, -6470.0),
        Kind::Container(1),
    );
    lamp(&mut p, &mut lamps, -200.0, -6900.0, 320.0);
    lamp(&mut p, &mut lamps, 200.0, -6900.0, 320.0);

    Level {
        pieces: p,
        spawn: v(0.0, 1.0, 800.0),
        facing: 0.0,
        hunters: vec![
            v(-950.0, 0.0, -1400.0),
            v(950.0, 0.0, -1800.0),
            v(0.0, 0.0, -2400.0),
            v(-1250.0, 204.0, -2700.0),
            v(-400.0, 0.0, -4300.0),
            v(300.0, 0.0, -4600.0),
            v(0.0, 444.0, -4400.0),
            v(1300.0, 64.0, -4000.0),
            v(-600.0, 0.0, -5950.0),
            v(900.0, 0.0, -5950.0),
        ],
        dummies: Vec::new(),
        exit: Some(v(0.0, 0.0, -6900.0)),
        lamps,
        mood: Mood {
            sky: Color::srgb(0.30, 0.17, 0.12),
            sun: Color::srgb(1.0, 0.68, 0.45),
            sun_strength: 3800.0,
            sun_direction: v(0.6, -0.55, -0.5),
            ambient: Color::srgb(0.95, 0.72, 0.6),
            ambient_strength: 600.0,
            tint: Color::srgb(1.0, 0.96, 0.92),
            lamp: Color::srgb(1.0, 0.8, 0.5),
        },
    }
}

/// The west-side roofs on the way up to the clock tower, each one higher than
/// the last: (from z, to z, height).
const WEST_ROOFS: [(f32, f32, f32); 6] = [
    (1000.0, -400.0, 240.0),
    (-560.0, -1500.0, 380.0),
    (-1660.0, -2600.0, 520.0),
    (-2760.0, -3700.0, 660.0),
    (-3860.0, -4800.0, 800.0),
    (-4960.0, -6400.0, 940.0),
];
const TOWER_HEIGHT: f32 = 1300.0;

/// A city block at night: a street between brick buildings, rooftops climbing
/// one after another up to the clock tower, where the exit is.
fn rooftops() -> Level {
    let mut p = Vec::new();
    let mut lamps = Vec::new();
    let (x0, z0, x1, z1) = (-1600.0, 1000.0, 1600.0, -7000.0);
    ground(&mut p, x0, z1, x1, z0);
    perimeter(&mut p, x0, z0, x1, z1, 1600.0);

    // West side: each roof a jump higher than the one before.
    for (near, far, height) in WEST_ROOFS {
        building(&mut p, -1600.0, near, -420.0, far, height);
    }
    // East side: whatever heights.
    for (near, far, height) in [
        (1000.0, -900.0, 420.0),
        (-1060.0, -2400.0, 300.0),
        (-2560.0, -3900.0, 700.0),
        (-4060.0, -5400.0, 460.0),
        (-5560.0, -7000.0, 600.0),
    ] {
        building(&mut p, 420.0, near, 1600.0, far, height);
    }
    // The clock tower, closing off the street, and its clock.
    building(&mut p, -300.0, -6300.0, 300.0, -6900.0, TOWER_HEIGHT);
    put(
        &mut p,
        v(-110.0, 1050.0, -6300.0),
        v(110.0, 1270.0, -6288.0),
        Kind::Marker,
    );

    // Up onto the first roof: a dumpster, then a fire-escape landing.
    put(
        &mut p,
        v(-420.0, 0.0, 140.0),
        v(-330.0, 80.0, 300.0),
        Kind::Container(2),
    );
    put(
        &mut p,
        v(-420.0, 140.0, -60.0),
        v(-320.0, 160.0, 100.0),
        Kind::Platform,
    );
    // On the last west roof: a hut and a vent, steps up to the tower.
    let top = WEST_ROOFS[5].2 + 10.0;
    put(
        &mut p,
        v(-1000.0, top, -6300.0),
        v(-700.0, top + 140.0, -6000.0),
        Kind::Wall,
    );
    put(
        &mut p,
        v(-640.0, top, -6260.0),
        v(-480.0, top + 280.0, -6060.0),
        Kind::Iron,
    );
    // A footbridge over the street.
    put(
        &mut p,
        v(-420.0, 670.0, -3360.0),
        v(420.0, 690.0, -3200.0),
        Kind::Platform,
    );
    // A billboard on a low roof.
    put(
        &mut p,
        v(700.0, 310.0, -1700.0),
        v(1300.0, 500.0, -1680.0),
        Kind::Crate,
    );

    for (x, z, paint) in [
        (-200.0, -900.0, 0),
        (220.0, -1900.0, 1),
        (-150.0, -3000.0, 3),
        (200.0, -4200.0, 2),
        (-220.0, -5200.0, 0),
        (150.0, -5900.0, 1),
    ] {
        car(&mut p, x, z, paint);
    }
    for (i, z) in [400.0, -1200.0, -2800.0, -4400.0, -5800.0]
        .into_iter()
        .enumerate()
    {
        let x = if i % 2 == 0 { -380.0 } else { 380.0 };
        lamp(&mut p, &mut lamps, x, z, 260.0);
    }
    // The tower's top, lit.
    lamps.push(v(0.0, TOWER_HEIGHT + 120.0, -6600.0));

    Level {
        pieces: p,
        spawn: v(0.0, 1.0, 700.0),
        facing: 0.0,
        hunters: vec![
            v(0.0, 0.0, -1300.0),
            v(-150.0, 0.0, -2400.0),
            v(150.0, 0.0, -4600.0),
            v(-1000.0, 390.0, -1000.0),
            v(-1000.0, 670.0, -3200.0),
            v(-900.0, 810.0, -4300.0),
            v(1000.0, 430.0, -200.0),
            v(1000.0, 710.0, -3000.0),
            v(1000.0, 610.0, -6200.0),
            v(-1200.0, 950.0, -5400.0),
        ],
        dummies: Vec::new(),
        exit: Some(v(0.0, TOWER_HEIGHT + 10.0, -6600.0)),
        lamps,
        mood: Mood {
            sky: Color::srgb(0.06, 0.07, 0.12),
            sun: Color::srgb(0.6, 0.7, 1.0),
            sun_strength: 1600.0,
            sun_direction: v(-0.3, -1.0, 0.35),
            ambient: Color::srgb(0.55, 0.6, 0.85),
            ambient_strength: 380.0,
            tint: Color::srgb(0.9, 0.92, 1.0),
            lamp: Color::srgb(1.0, 0.75, 0.4),
        },
    }
}

/// How deep the foundry's pit is.
const PIT_DEPTH: f32 = 320.0;
/// The height of the foundry's catwalks.
const CATWALK: f32 = 384.0;

/// A foundry: catwalks round the walls, a furnace in a pit in the middle, and
/// the exit down in the pit beside it.
fn foundry() -> Level {
    let mut p = Vec::new();
    let mut lamps = Vec::new();
    let (x0, z0, x1, z1) = (-2400.0, 1200.0, 2400.0, -5200.0);
    let pit = (-900.0, -1200.0, 900.0, -3200.0);
    // The floor in four thick slabs round the pit, and the pit's floor.
    let deep = -PIT_DEPTH - 80.0;
    put(
        &mut p,
        v(x0 - 200.0, deep, pit.1),
        v(x1 + 200.0, 0.0, z0 + 200.0),
        Kind::Floor,
    );
    put(
        &mut p,
        v(x0 - 200.0, deep, z1 - 200.0),
        v(x1 + 200.0, 0.0, pit.3),
        Kind::Floor,
    );
    put(
        &mut p,
        v(x0 - 200.0, deep, pit.3),
        v(pit.0, 0.0, pit.1),
        Kind::Floor,
    );
    put(
        &mut p,
        v(pit.2, deep, pit.3),
        v(x1 + 200.0, 0.0, pit.1),
        Kind::Floor,
    );
    put(
        &mut p,
        v(pit.0, deep - 20.0, pit.3),
        v(pit.2, -PIT_DEPTH, pit.1),
        Kind::Step,
    );
    perimeter(&mut p, x0, z0, x1, z1, 1400.0);

    // Ramps down into the pit from the south and the north.
    p.push(map::slope(
        v(-500.0, -PIT_DEPTH, -1840.0),
        v(-500.0, 0.0, pit.1),
        240.0,
        Kind::Ramp,
    ));
    p.push(map::slope(
        v(500.0, -PIT_DEPTH, -2560.0),
        v(500.0, 0.0, pit.3),
        240.0,
        Kind::Ramp,
    ));
    // The furnace and its chimney.
    put(
        &mut p,
        v(-220.0, -PIT_DEPTH, -2420.0),
        v(220.0, 420.0, -1980.0),
        Kind::Iron,
    );
    put(
        &mut p,
        v(-100.0, 420.0, -2300.0),
        v(100.0, 1100.0, -2100.0),
        Kind::Iron,
    );
    lamps.push(v(-380.0, -150.0, -2200.0));
    lamps.push(v(380.0, -150.0, -2200.0));
    lamps.push(v(0.0, -150.0, -2700.0));

    // Catwalks round three sides, stairs up at the south ends, and the
    // pillars holding them up.
    for x in [-1640.0, 1640.0] {
        map::stairs(&mut p, x, 0.0, 16.0, (CATWALK / 16.0) as usize, 160.0);
        put(
            &mut p,
            v(x - 80.0, CATWALK - 20.0, -928.0),
            v(x + 80.0, CATWALK, -3600.0),
            Kind::Platform,
        );
        for z in [-1700.0, -2600.0, -3500.0] {
            put(
                &mut p,
                v(x - 20.0, 0.0, z - 20.0),
                v(x + 20.0, CATWALK - 20.0, z + 20.0),
                Kind::Iron,
            );
        }
        lamps.push(v(x, CATWALK + 200.0, -2200.0));
    }
    put(
        &mut p,
        v(-1720.0, CATWALK - 20.0, -3760.0),
        v(1720.0, CATWALK, -3600.0),
        Kind::Platform,
    );
    for x in [-800.0, 0.0, 800.0] {
        put(
            &mut p,
            v(x - 20.0, 0.0, -3700.0),
            v(x + 20.0, CATWALK - 20.0, -3660.0),
            Kind::Iron,
        );
    }

    // Hanging platforms, up high for the hook.
    for (x, y, z) in [
        (-700.0, 950.0, -2200.0),
        (1200.0, 800.0, -4400.0),
        (-1200.0, 800.0, -400.0),
    ] {
        put(
            &mut p,
            v(x - 150.0, y, z - 150.0),
            v(x + 150.0, y + 30.0, z + 150.0),
            Kind::Platform,
        );
    }
    // Low walls to hide behind near the start, crates of ore, and two long
    // walls at the back for wall-running.
    put(
        &mut p,
        v(-1400.0, 0.0, -640.0),
        v(-600.0, 160.0, -600.0),
        Kind::Wall,
    );
    put(
        &mut p,
        v(600.0, 0.0, -640.0),
        v(1400.0, 160.0, -600.0),
        Kind::Wall,
    );
    for (x, z, size) in [
        (-900.0, 200.0, 96.0),
        (1000.0, 100.0, 96.0),
        (-1100.0, -4200.0, 96.0),
        (900.0, -4700.0, 64.0),
        (0.0, -4600.0, 96.0),
    ] {
        crate_at(&mut p, x, 0.0, z, size);
    }
    put(
        &mut p,
        v(-1020.0, 0.0, -5000.0),
        v(-980.0, 400.0, -3900.0),
        Kind::Wall,
    );
    put(
        &mut p,
        v(980.0, 0.0, -5000.0),
        v(1020.0, 400.0, -3900.0),
        Kind::Wall,
    );

    Level {
        pieces: p,
        spawn: v(0.0, 1.0, 900.0),
        facing: 0.0,
        hunters: vec![
            v(-1100.0, 0.0, -900.0),
            v(1100.0, 0.0, -900.0),
            v(0.0, 0.0, -700.0),
            v(-500.0, -PIT_DEPTH, -2600.0),
            v(500.0, -PIT_DEPTH, -1700.0),
            v(-300.0, -PIT_DEPTH, -2900.0),
            v(-1640.0, CATWALK, -2200.0),
            v(1640.0, CATWALK, -2800.0),
            v(0.0, CATWALK, -3680.0),
            v(-600.0, 0.0, -4300.0),
            v(600.0, 0.0, -4600.0),
            v(0.0, 0.0, -4950.0),
        ],
        dummies: Vec::new(),
        exit: Some(v(0.0, -PIT_DEPTH, -2850.0)),
        lamps,
        mood: Mood {
            sky: Color::srgb(0.22, 0.08, 0.03),
            sun: Color::srgb(1.0, 0.55, 0.3),
            sun_strength: 2600.0,
            sun_direction: v(0.25, -1.0, -0.3),
            ambient: Color::srgb(1.0, 0.62, 0.42),
            ambient_strength: 520.0,
            tint: Color::srgb(1.0, 0.92, 0.86),
            lamp: Color::srgb(1.0, 0.45, 0.15),
        },
    }
}

// ---- loading ----

/// Brightness of a lamp, lumens (our units are inches, so it's big), and how
/// far its light reaches.
const LAMP_LIGHT: f32 = 2.5e9;
const LAMP_RANGE: f32 = 1400.0;

#[allow(clippy::too_many_arguments)]
fn load_level(
    mut commands: Commands,
    mut loads: MessageReader<LoadLevel>,
    things: Query<Entity, With<LevelThing>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    kit: Res<CharacterKit>,
    look: Res<ExitLook>,
    mut map: ResMut<MapCollision>,
    mut sky: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut fogs: Query<&mut DistanceFog>,
    mut current: ResMut<CurrentLevel>,
    mut stats: ResMut<LevelStats>,
    mut respawn: MessageWriter<RespawnPlayer>,
) {
    let Some(LoadLevel(index)) = loads.read().last().copied() else {
        return;
    };
    let index = index.min(COUNT - 1);
    for thing in &things {
        commands.entity(thing).despawn();
    }
    let level = level(index);
    let mood = &level.mood;

    map::spawn_pieces(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        &level.pieces,
        mood.tint,
    );
    map.0 = level.collision();
    sky.0 = mood.sky;
    for mut fog in &mut fogs {
        fog.color = mood.sky;
    }
    ambient.color = mood.ambient;
    ambient.brightness = mood.ambient_strength;
    commands.spawn((
        LevelThing,
        DirectionalLight {
            illuminance: mood.sun_strength,
            color: mood.sun,
            shadow_maps_enabled: true,
            ..default()
        },
        // Lights the world and the first-person gun alike.
        RenderLayers::from_layers(&[0, VIEW_MODEL_LAYER]),
        Transform::default().looking_to(mood.sun_direction, Vec3::Y),
        // Default shadow distances assume 1 unit = 1 metre; ours are inches.
        bevy::light::CascadeShadowConfigBuilder {
            first_cascade_far_bound: 600.0,
            maximum_distance: 6000.0,
            ..default()
        }
        .build(),
    ));
    for &at in &level.lamps {
        commands.spawn((
            LevelThing,
            PointLight {
                intensity: LAMP_LIGHT,
                range: LAMP_RANGE,
                color: mood.lamp,
                ..default()
            },
            Transform::from_translation(at),
        ));
    }

    // In training, the hunters get back up.
    let respawns = index == TRAINING;
    for (i, &post) in level.hunters.iter().enumerate() {
        let hunter = enemies::spawn_hunter(&mut commands, &kit, &mut materials, post, i, respawns);
        commands.entity(hunter).insert(LevelThing);
    }
    for &feet in &level.dummies {
        let dummy = targets::spawn_dummy(&mut commands, &mut meshes, &mut materials, feet);
        commands.entity(dummy).insert(LevelThing);
    }
    if let Some(exit) = level.exit {
        spawn_exit(&mut commands, &look, exit);
    }

    *current = CurrentLevel {
        index,
        spawn: level.spawn,
        facing: level.facing,
        exit: level.exit,
        hunters: level.hunters.len(),
        hunters_left: level.hunters.len(),
        cleared: false,
    };
    *stats = LevelStats::default();
    respawn.write(RespawnPlayer);
}

// ---- the exit ----

/// The exit: a pad with a column of light over it. Dark until every hunter is
/// dead, then it glows and throbs.
#[derive(Component)]
struct Exit {
    open: bool,
}

/// The beacon's column, which throbs once the exit is open.
#[derive(Component)]
struct Beacon;

#[derive(Resource)]
struct ExitLook {
    cube: Handle<Mesh>,
    pad: Handle<StandardMaterial>,
    shut: Handle<StandardMaterial>,
    open: Handle<StandardMaterial>,
}

/// How close (across) you need to get to the exit's middle, units.
const EXIT_RADIUS: f32 = 110.0;
const BEACON_HEIGHT: f32 = 3000.0;

fn make_exit_look(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut glow = |color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            unlit: true,
            ..default()
        })
    };
    commands.insert_resource(ExitLook {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        pad: glow(Color::srgb(0.15, 0.05, 0.04)),
        shut: glow(Color::srgb(0.22, 0.05, 0.05)),
        open: glow(Color::srgb(1.0, 0.82, 0.45)),
    });
}

fn spawn_exit(commands: &mut Commands, look: &ExitLook, at: Vec3) {
    commands
        .spawn((
            LevelThing,
            Exit { open: false },
            Transform::from_translation(at),
            Visibility::default(),
        ))
        .with_children(|exit| {
            exit.spawn((
                Mesh3d(look.cube.clone()),
                MeshMaterial3d(look.pad.clone()),
                Transform::from_xyz(0.0, 2.0, 0.0).with_scale(v(
                    EXIT_RADIUS * 2.0,
                    4.0,
                    EXIT_RADIUS * 2.0,
                )),
                NotShadowCaster,
            ));
            exit.spawn((
                Beacon,
                Mesh3d(look.cube.clone()),
                MeshMaterial3d(look.shut.clone()),
                Transform::from_xyz(0.0, BEACON_HEIGHT * 0.5, 0.0).with_scale(v(
                    40.0,
                    BEACON_HEIGHT,
                    40.0,
                )),
                NotShadowCaster,
            ));
        });
}

/// Count the hunters still standing, open the exit when there are none, and
/// clear the level when the player gets to it. Also keeps the level's clock.
#[allow(clippy::too_many_arguments)]
fn watch_objective(
    time: Res<Time>,
    run: Res<Run>,
    player: Res<PlayerStatus>,
    hunters: Query<&Enemy>,
    mut exits: Query<&mut Exit>,
    mut current: ResMut<CurrentLevel>,
    mut stats: ResMut<LevelStats>,
    mut kills: MessageReader<EnemyKilled>,
    mut cleared: MessageWriter<LevelCleared>,
) {
    let killed = kills.read().count() as u32;
    if !run.started {
        return;
    }
    stats.time += time.delta_secs();
    stats.kills += killed;
    current.hunters_left = hunters.iter().filter(|h| h.alive()).count();
    let open = current.hunters_left == 0;
    for mut exit in &mut exits {
        exit.open = open;
    }
    let Some(exit) = current.exit else {
        return;
    };
    let across = Vec2::new(player.position.x - exit.x, player.position.z - exit.z).length();
    let up = player.position.y - exit.y;
    if open && !current.cleared && across < EXIT_RADIUS && (-20.0..250.0).contains(&up) {
        current.cleared = true;
        cleared.write(LevelCleared);
    }
}

/// Light the beacon once the exit opens, and make it throb.
fn light_exit(
    real: Res<Time<Real>>,
    look: Res<ExitLook>,
    exits: Query<(&Exit, &Children)>,
    mut beacons: Query<(&mut MeshMaterial3d<StandardMaterial>, &mut Transform), With<Beacon>>,
) {
    let throb = 1.0 + 0.6 * (real.elapsed_secs() * PI * 2.0).sin().max(0.0);
    for (exit, children) in &exits {
        for &child in &**children {
            let Ok((mut material, mut transform)) = beacons.get_mut(child) else {
                continue;
            };
            let (wanted, width) = if exit.open {
                (&look.open, 70.0 * throb)
            } else {
                (&look.shut, 40.0)
            };
            if material.0 != *wanted {
                material.0 = wanted.clone();
            }
            transform.scale.x = width;
            transform.scale.z = width;
        }
    }
}

// ---- HUD ----

#[derive(Component)]
struct ObjectiveText;

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        ObjectiveText,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(20.0),
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.85, 0.7)),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            // Below the movement readout in the top left.
            top: px(70),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        },
    ));
}

/// "m:ss".
pub fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Which way to turn to face something `bearing` radians round from where you
/// look (the same way as yaw: positive is to the left).
fn which_way(bearing: f32) -> &'static str {
    let b = (bearing + PI).rem_euclid(2.0 * PI) - PI;
    if b.abs() < 0.4 {
        "AHEAD"
    } else if b.abs() > 2.6 {
        "BEHIND"
    } else if b > 0.0 {
        "LEFT"
    } else {
        "RIGHT"
    }
}

fn update_hud(
    real: Res<Time<Real>>,
    run: Res<Run>,
    current: Res<CurrentLevel>,
    stats: Res<LevelStats>,
    player: Res<PlayerStatus>,
    mut text: Single<(&mut Text, &mut TextColor), With<ObjectiveText>>,
) {
    let (text, colour) = &mut *text;
    if !run.started {
        text.0.clear();
        return;
    }
    let (name, _) = info(current.index);
    if current.index == TRAINING {
        text.0 = format!("{name}   Hunters get back up here. Esc for the menu.");
        colour.0 = Color::srgb(0.85, 0.8, 0.7);
        return;
    }
    let time = clock(stats.time);
    match current.exit {
        Some(exit) if current.hunters_left == 0 => {
            let to = exit - player.position;
            let bearing = (-to.x).atan2(-to.z) - player.yaw;
            let mut way = which_way(bearing).to_string();
            if to.y > 150.0 {
                way += ", UP";
            } else if to.y < -150.0 {
                way += ", DOWN";
            }
            let metres = Vec2::new(to.x, to.z).length() * 0.0254;
            text.0 = format!("{name}   EXIT OPEN: {way} {metres:.0} m   {time}");
            let blink = (real.elapsed_secs() * 3.0).fract() < 0.5;
            colour.0 = if blink {
                Color::srgb(1.0, 0.85, 0.45)
            } else {
                Color::srgb(0.95, 0.65, 0.3)
            };
        }
        _ => {
            text.0 = format!(
                "{name}   HUNTERS LEFT {} / {}   {time}",
                current.hunters_left, current.hunters
            );
            colour.0 = Color::srgb(0.95, 0.85, 0.7);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpdump_movement::{MoveInput, MovementState, MovementTuning, WallKind, step};

    const DT: f32 = 1.0 / 60.0;

    fn tuning() -> MovementTuning {
        ron::from_str(include_str!("../assets/movement.ron")).unwrap()
    }

    /// Drop a player from `at` and let them settle for `seconds`.
    fn drop_from(
        at: Vec3,
        world: &pumpdump_movement::StaticWorld,
        tuning: &MovementTuning,
        seconds: f32,
    ) -> MovementState {
        let mut state = MovementState::new(at);
        for _ in 0..(seconds / DT) as usize {
            state = step(&state, &MoveInput::default(), tuning, world, DT);
        }
        state
    }

    #[test]
    fn every_level_has_a_name() {
        let names: Vec<_> = (0..COUNT).map(|i| info(i).0).collect();
        for (i, name) in names.iter().enumerate() {
            assert!(!name.is_empty());
            assert!(!names[..i].contains(name), "{name} twice");
        }
    }

    /// With no input, the player lands at the start of every level and stays
    /// there.
    #[test]
    fn standing_at_every_start_stays_put() {
        let tuning = tuning();
        for index in 0..COUNT {
            let level = level(index);
            let state = drop_from(level.spawn, &level.collision(), &tuning, 5.0);
            let drift = Vec2::new(
                state.position.x - level.spawn.x,
                state.position.z - level.spawn.z,
            );
            assert!(state.on_ground, "{}: {state:?}", info(index).0);
            assert!(drift.length() < 0.01, "{}: {state:?}", info(index).0);
            assert!((state.position.y - level.spawn.y).abs() < 2.0, "{state:?}");
        }
    }

    /// Every hunter's post is somewhere to stand: dropped there, they land
    /// close below it, not inside anything or falling through.
    #[test]
    fn every_hunter_stands_on_something() {
        let tuning = tuning();
        for index in 0..COUNT {
            let level = level(index);
            let world = level.collision();
            for &post in &level.hunters {
                let state = drop_from(post + Vec3::Y, &world, &tuning, 2.0);
                assert!(
                    state.on_ground && (state.position.y - post.y).abs() < 4.0,
                    "{} hunter at {post}: {state:?}",
                    info(index).0
                );
            }
        }
    }

    /// Every campaign level has an exit you can stand at.
    #[test]
    fn every_exit_is_somewhere_to_stand() {
        let tuning = tuning();
        for index in 1..COUNT {
            let level = level(index);
            let exit = level.exit.expect("campaign levels have exits");
            let state = drop_from(exit + Vec3::Y * 40.0, &level.collision(), &tuning, 2.0);
            assert!(
                state.on_ground && (state.position.y - exit.y).abs() < 4.0,
                "{}: {state:?}",
                info(index).0
            );
        }
        assert!(level(TRAINING).exit.is_none());
    }

    /// The rooftops can be climbed without the hook: every roof on the way up
    /// to the tower is within a double jump of the one before.
    #[test]
    fn the_rooftops_climb_a_jump_at_a_time() {
        let tuning = tuning();
        let reach = tuning.jump_height + tuning.air_jump_height;
        let level = rooftops();
        let world = level.collision();
        let mut heights = vec![0.0];
        // The dumpster and the fire escape, then each west roof.
        for at in [v(-375.0, 0.0, 220.0), v(-370.0, 0.0, 20.0)] {
            heights.push(
                drop_from(at + Vec3::Y * 2000.0, &world, &tuning, 3.0)
                    .position
                    .y,
            );
        }
        for (near, far, _) in WEST_ROOFS {
            let at = v(-1000.0, 2000.0, (near + far) * 0.5);
            heights.push(drop_from(at, &world, &tuning, 3.0).position.y);
        }
        // The hut, the vent and the tower.
        for at in [
            v(-850.0, 0.0, -6150.0),
            v(-560.0, 0.0, -6160.0),
            v(0.0, 0.0, -6600.0),
        ] {
            heights.push(
                drop_from(at + Vec3::Y * 2000.0, &world, &tuning, 3.0)
                    .position
                    .y,
            );
        }
        for pair in heights.windows(2) {
            let climb = pair[1] - pair[0];
            assert!(climb > 0.0 && climb < reach - 20.0, "{heights:?}");
        }
        assert!((heights.last().unwrap() - (TOWER_HEIGHT + 10.0)).abs() < 1.0);
    }

    #[test]
    fn which_way_points_the_right_way() {
        assert_eq!(which_way(0.1), "AHEAD");
        assert_eq!(which_way(1.2), "LEFT");
        assert_eq!(which_way(-1.2), "RIGHT");
        assert_eq!(which_way(3.0), "BEHIND");
        // Wraps round.
        assert_eq!(which_way(2.0 * PI + 0.1), "AHEAD");
        assert_eq!(clock(75.4), "1:15");
    }

    /// With the shipped tuning, a training corridor wall-run lasts until the
    /// wall runs out (it's shorter than a full-length run), without sinking to
    /// the floor.
    #[test]
    fn corridor_wall_runs_last_to_the_end_of_the_wall() {
        let tuning = tuning();
        let world = training().collision();
        // Beside the right corridor wall (inner face x = 150), jumping in at sprint.
        let mut state = MovementState::new(v(150.0 - 16.0 - 2.0, 100.0, -1500.0));
        state.velocity = v(0.0, 300.0, -tuning.walk_speed);
        let forward = MoveInput {
            wish: Vec2::Y,
            sprint: true,
            ..Default::default()
        };
        let mut ran = 0.0;
        for _ in 0..(tuning.wall_run_max_time / DT) as usize + 60 {
            state = step(&state, &forward, &tuning, &world, DT);
            match state.wall {
                Some(wall) if wall.kind == WallKind::Run => ran = wall.time,
                _ if ran > 0.0 => break,
                _ => {}
            }
        }
        // The wall ends at z = -5400, about 3900 units on: roughly 5 s at run speed.
        assert!(ran > 4.0, "ran {ran} s: {state:?}");
        assert!(state.position.z < -5300.0, "fell off early: {state:?}");
    }

    /// Slide-hopping down training's open floor with the shipped tuning builds
    /// speed well past a plain slide, and never past the cap.
    #[test]
    fn slide_hopping_builds_speed_under_the_cap() {
        let tuning = tuning();
        let world = training().collision();
        // Start far from everything, on the open floor behind the spawn, facing +X.
        let mut state = MovementState::new(v(-5500.0, 1.0, 2500.0));
        let hop = MoveInput {
            wish: Vec2::Y,
            yaw: -std::f32::consts::FRAC_PI_2,
            sprint: true,
            slide: true,
            jump: true,
            ..Default::default()
        };
        let mut fastest = 0.0f32;
        // Stop well before the floor's far edge (x = 6000).
        while state.position.x < 5000.0 {
            state = step(&state, &hop, &tuning, &world, DT);
            assert!(state.position.y > -1.0, "fell off: {state:?}");
            fastest = fastest.max(Vec2::new(state.velocity.x, state.velocity.z).length());
        }
        assert!(fastest <= tuning.max_speed + 1e-2, "{fastest}");
        assert!(fastest > tuning.slide_speed * 1.5, "only reached {fastest}");
    }
}
