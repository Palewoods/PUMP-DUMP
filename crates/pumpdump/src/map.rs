//! The greybox test map, built in code.
//!
//! Everything is a box, so one list of [`Piece`]s gives both the visible meshes
//! and the collision world. Ramps are tilted slabs whose low end is sunk into the
//! floor. Units are game units (~1 inch), Y up. The player spawns at the origin
//! facing -Z.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use pumpdump_movement::StaticWorld;

use crate::retro::VIEW_MODEL_LAYER;

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        // Rust note: `insert_resource` stores one global value of a type. Systems
        // ask for it with `Res<MapCollision>`.
        app.insert_resource(MapCollision(collision(&layout())))
            .insert_resource(ClearColor(SKY))
            // Dim, warm fill light so faces turned away from the sun aren't black.
            .insert_resource(GlobalAmbientLight {
                color: Color::srgb(0.95, 0.78, 0.65),
                brightness: 650.0,
                ..default()
            })
            .add_systems(Startup, spawn_map);
    }
}

/// The collision side of the map, read by the movement tick.
#[derive(Resource)]
pub struct MapCollision(pub StaticWorld);

/// Where the player starts and resets to.
pub const SPAWN: Vec3 = Vec3::new(0.0, 1.0, 0.0);

/// Murky dusk. Also the distance fog's colour, so far things fade into it.
pub const SKY: Color = Color::srgb(0.24, 0.17, 0.14);

/// Every surface texture repeats every this many units.
const TEXTURE_SPAN: f32 = 128.0;
/// Surface textures are this many pixels square: chunky on purpose.
const TEXELS: u32 = 32;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Floor,
    Wall,
    Ramp,
    Step,
    Crate,
    Marker,
    /// Floating platforms, up high for the grappling hook.
    Platform,
}

impl Kind {
    /// Tint over the (grey) texture: dusty, rusty, muted.
    fn color(self) -> Color {
        match self {
            Kind::Floor => Color::srgb(0.40, 0.35, 0.30),
            Kind::Wall => Color::srgb(0.55, 0.36, 0.28),
            Kind::Ramp => Color::srgb(0.48, 0.33, 0.20),
            Kind::Step => Color::srgb(0.42, 0.44, 0.36),
            Kind::Crate => Color::srgb(0.60, 0.45, 0.28),
            Kind::Marker => Color::srgb(0.80, 0.62, 0.22),
            Kind::Platform => Color::srgb(0.36, 0.40, 0.42),
        }
    }

    fn pattern(self) -> Pattern {
        match self {
            Kind::Floor => Pattern::Slabs,
            Kind::Wall => Pattern::Bricks,
            Kind::Ramp | Kind::Crate => Pattern::Planks,
            Kind::Step => Pattern::Concrete,
            Kind::Marker => Pattern::Plain,
            Kind::Platform => Pattern::Slabs,
        }
    }
}

/// The grey pattern each kind of surface is textured with.
#[derive(Clone, Copy)]
enum Pattern {
    /// Big stone floor slabs.
    Slabs,
    Bricks,
    Planks,
    /// Blotchy, cracked concrete.
    Concrete,
    /// Just grain.
    Plain,
}

struct Piece {
    center: Vec3,
    half: Vec3,
    rotation: Quat,
    kind: Kind,
}

/// An axis-aligned box from corner `min` to corner `max`.
fn block(min: Vec3, max: Vec3, kind: Kind) -> Piece {
    Piece {
        center: (min + max) * 0.5,
        half: (max - min) * 0.5,
        rotation: Quat::IDENTITY,
        kind,
    }
}

/// A ramp `width` wide centred on `x`, rising towards -Z from the floor at `z0`
/// over `run` units at `degrees`, with a platform at the top.
fn ramp(pieces: &mut Vec<Piece>, x: f32, z0: f32, run: f32, degrees: f32, width: f32) {
    const THICKNESS: f32 = 16.0;
    const SUNK: f32 = 32.0; // extra slab length hidden below the floor
    let angle = degrees.to_radians();
    let rise = run * angle.tan();
    // The slab's top face runs from below the floor up to (z0 - run, rise).
    let low = Vec3::new(x, -SUNK * angle.sin(), z0 + SUNK * angle.cos());
    let high = Vec3::new(x, rise, z0 - run);
    let rotation = Quat::from_rotation_x(angle);
    let up = rotation * Vec3::Y;
    pieces.push(Piece {
        center: (low + high) * 0.5 - up * (THICKNESS * 0.5),
        half: Vec3::new(width * 0.5, THICKNESS * 0.5, low.distance(high) * 0.5),
        rotation,
        kind: Kind::Ramp,
    });
    let half_w = width * 0.5;
    pieces.push(block(
        Vec3::new(x - half_w, 0.0, z0 - run - 240.0),
        Vec3::new(x + half_w, rise, z0 - run),
        Kind::Ramp,
    ));
}

/// `count` steps of `rise` each, 32 deep, `width` wide, climbing towards -Z from
/// `z0`, with a landing at the top.
fn stairs(pieces: &mut Vec<Piece>, x: f32, z0: f32, rise: f32, count: usize, width: f32) {
    const DEPTH: f32 = 32.0;
    let back = z0 - DEPTH * count as f32 - 160.0;
    let half_w = width * 0.5;
    for k in 0..count {
        let front = z0 - DEPTH * k as f32;
        let top = rise * (k + 1) as f32;
        pieces.push(block(
            Vec3::new(x - half_w, 0.0, back),
            Vec3::new(x + half_w, top, front),
            Kind::Step,
        ));
    }
}

fn layout() -> Vec<Piece> {
    let mut p = Vec::new();

    // Floor, top at y = 0.
    p.push(block(
        Vec3::new(-6000.0, -16.0, -6000.0),
        Vec3::new(6000.0, 0.0, 3000.0),
        Kind::Floor,
    ));

    // Ramps on the right: walkable 15°, 30°, 40°; too steep 55°. Max slope is in movement.ron.
    for (i, degrees) in [15.0, 30.0, 40.0, 55.0].into_iter().enumerate() {
        ramp(
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
        stairs(&mut p, -450.0 - 200.0 * i as f32, -300.0, rise, 8, 160.0);
    }

    // Behind the spawn: single ledges from walk-up-able to jump-only.
    for (i, height) in [12.0, 18.0, 20.0, 24.0, 32.0, 48.0, 64.0]
        .into_iter()
        .enumerate()
    {
        let x = -540.0 + 160.0 * i as f32;
        p.push(block(
            Vec3::new(x - 48.0, 0.0, 300.0),
            Vec3::new(x + 48.0, height, 400.0),
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
        let h = size * 0.5;
        p.push(block(
            Vec3::new(x - h, 0.0, z - h),
            Vec3::new(x + h, size, z + h),
            Kind::Crate,
        ));
    }

    // Speed lane: a marker post every 100 units along x = 260, from the spawn line.
    for k in 0..14 {
        let z = -100.0 * k as f32;
        p.push(block(
            Vec3::new(258.0, 0.0, z - 2.0),
            Vec3::new(262.0, 40.0, z + 2.0),
            Kind::Marker,
        ));
    }

    // Long walls for wall-running: a corridor 300 wide, a lone wall, and a
    // staggered pair for chaining runs.
    let wall = |x: f32, z_from: f32, z_to: f32, height: f32| {
        block(
            Vec3::new(x - 8.0, 0.0, z_to),
            Vec3::new(x + 8.0, height, z_from),
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
            Vec3::new(x - h, y, z - h),
            Vec3::new(x + h, y + 40.0, z + h),
            Kind::Platform,
        ));
    }

    p
}

fn collision(pieces: &[Piece]) -> StaticWorld {
    let mut world = StaticWorld::new();
    for piece in pieces {
        world.add_box(piece.center, piece.half, piece.rotation);
    }
    world
}

fn spawn_map(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut material_for = std::collections::HashMap::new();

    for piece in layout() {
        let material = material_for
            .entry(piece.kind)
            .or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: piece.kind.color(),
                    base_color_texture: Some(images.add(texture(piece.kind.pattern()))),
                    // Flat and matte, no shine: an old-school look.
                    perceptual_roughness: 1.0,
                    reflectance: 0.05,
                    ..default()
                })
            })
            .clone();
        // Bevy note: an entity is just an ID; what it *is* comes from the
        // components spawned on it. Mesh + material + transform = a visible object.
        commands.spawn((
            Mesh3d(meshes.add(box_mesh(piece.half))),
            MeshMaterial3d(material),
            Transform::from_translation(piece.center).with_rotation(piece.rotation),
        ));
    }

    commands.spawn((
        DirectionalLight {
            illuminance: 4_200.0,
            color: Color::srgb(1.0, 0.82, 0.62),
            shadow_maps_enabled: true,
            ..default()
        },
        // Lights the world and the first-person gun alike.
        RenderLayers::from_layers(&[0, VIEW_MODEL_LAYER]),
        Transform::from_xyz(0.0, 0.0, 0.0).looking_to(Vec3::new(-0.4, -1.0, -0.6), Vec3::Y),
        // Default shadow distances assume 1 unit = 1 metre; ours are inches.
        bevy::light::CascadeShadowConfigBuilder {
            first_cascade_far_bound: 600.0,
            maximum_distance: 6000.0,
            ..default()
        }
        .build(),
    ));
}

/// A box mesh whose UVs are in world units, so textures are the same size on
/// every surface and show how fast you're moving.
fn box_mesh(half: Vec3) -> Mesh {
    let mut mesh = Mesh::from(Cuboid::from_size(half * 2.0));
    let (
        Some(VertexAttributeValues::Float32x3(positions)),
        Some(VertexAttributeValues::Float32x3(normals)),
    ) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
    )
    else {
        return mesh;
    };
    let uvs: Vec<[f32; 2]> = positions
        .iter()
        .zip(normals)
        .map(|(p, n)| {
            let n = Vec3::from(*n).abs();
            let uv = if n.x > 0.5 {
                [p[2], p[1]]
            } else if n.y > 0.5 {
                [p[0], p[2]]
            } else {
                [p[0], p[1]]
            };
            [uv[0] / TEXTURE_SPAN, uv[1] / TEXTURE_SPAN]
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh
}

/// A small tiling grey texture for `pattern`, drawn pixel by pixel. Material
/// colours tint it. Nearest-neighbour sampling keeps the pixels blocky.
fn texture(pattern: Pattern) -> Image {
    let data: Vec<u8> = (0..TEXELS * TEXELS)
        .flat_map(|i| {
            let v = (shade(pattern, i % TEXELS, i / TEXELS).clamp(0.0, 1.0) * 255.0) as u8;
            [v, v, v, 255]
        })
        .collect();
    let mut image = Image::new(
        Extent3d {
            width: TEXELS,
            height: TEXELS,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::nearest()
    });
    image
}

/// Brightness of texture pixel (x, y), 0..1. Every pattern tiles seamlessly
/// because its blocks divide the texture size exactly.
fn shade(pattern: Pattern, x: u32, y: u32) -> f32 {
    let grain = noise(x, y, 1) - 0.5;
    match pattern {
        Pattern::Slabs => {
            if x.is_multiple_of(16) || y.is_multiple_of(16) {
                0.42 // the gaps between slabs
            } else {
                0.76 + 0.14 * noise(x / 16, y / 16, 2) + 0.12 * grain
            }
        }
        Pattern::Bricks => {
            // 16x8 bricks, every other row shifted by half a brick.
            let row = y / 8;
            let x = (x + if row.is_multiple_of(2) { 0 } else { 8 }) % TEXELS;
            if x.is_multiple_of(16) || y.is_multiple_of(8) {
                0.38 // mortar
            } else {
                0.72 + 0.18 * noise(x / 16, row, 3) + 0.12 * grain
            }
        }
        Pattern::Planks => {
            let row = y / 8;
            if y.is_multiple_of(8) {
                0.32 // seams
            } else {
                // Grain streaks run along the plank.
                0.66 + 0.14 * noise(0, row, 5) + 0.12 * noise(x / 4, row, 4) + 0.06 * grain
            }
        }
        Pattern::Concrete => {
            let crack = if noise(x, y / 3, 7) > 0.96 { 0.25 } else { 0.0 };
            0.72 + 0.2 * (noise(x / 2, y / 2, 6) - 0.5) + 0.1 * grain - crack
        }
        Pattern::Plain => 0.85 + 0.12 * grain,
    }
}

/// Repeatable pseudo-random value in 0..1 for a grid cell.
fn noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut h =
        x.wrapping_mul(0x27D4_EB2D) ^ y.wrapping_mul(0x1656_67B1) ^ seed.wrapping_mul(0x9E37_79B9);
    h = (h ^ (h >> 15)).wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    (h & 0xFFFF) as f32 / 65535.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpdump_movement::{MoveInput, MovementState, MovementTuning, WallKind, step};

    const DT: f32 = 1.0 / 60.0;

    fn tuning() -> MovementTuning {
        ron::from_str(include_str!("../assets/movement.ron")).unwrap()
    }

    /// The real map and the shipped tuning: with no input, the player lands at the
    /// spawn and stays there.
    #[test]
    fn standing_at_the_spawn_stays_put() {
        let tuning = tuning();
        let world = collision(&layout());
        let mut state = MovementState::new(SPAWN);
        for _ in 0..300 {
            state = step(&state, &MoveInput::default(), &tuning, &world, DT);
        }
        let drift = Vec2::new(state.position.x - SPAWN.x, state.position.z - SPAWN.z);
        assert!(state.on_ground, "{state:?}");
        assert!(drift.length() < 0.01, "{state:?}");
        assert_eq!(state.velocity, Vec3::ZERO);
    }

    /// With the shipped tuning, a corridor wall-run lasts until the wall runs out
    /// (it's shorter than a full-length run), without sinking to the floor.
    #[test]
    fn corridor_wall_runs_last_to_the_end_of_the_wall() {
        let tuning = tuning();
        let world = collision(&layout());
        // Beside the right corridor wall (inner face x = 150), jumping in at sprint.
        let mut state = MovementState::new(Vec3::new(150.0 - 16.0 - 2.0, 100.0, -1500.0));
        state.velocity = Vec3::new(0.0, 300.0, -tuning.walk_speed);
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

    /// Slide-hopping down the open floor with the shipped tuning builds speed well
    /// past a plain slide, and never past the cap.
    #[test]
    fn slide_hopping_builds_speed_under_the_cap() {
        let tuning = tuning();
        let world = collision(&layout());
        // Start far from everything, on the open floor behind the spawn, facing +X.
        let mut state = MovementState::new(Vec3::new(-5500.0, 1.0, 2500.0));
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
