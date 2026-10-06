//! Building blocks for the levels: boxes, ramps and stairs, their textures, and
//! the collision world made from them. The levels themselves are laid out in
//! `levels.rs`.
//!
//! Everything is a box, so one list of [`Piece`]s gives both the visible meshes
//! and the collision world. Ramps are tilted slabs whose low end is sunk into the
//! floor. Units are game units (~1 inch), Y up.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use pumpdump_movement::StaticWorld;

use crate::levels::LevelThing;

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        // Rust note: `insert_resource` stores one global value of a type. Systems
        // ask for it with `Res<MapCollision>`. Empty until a level loads.
        app.insert_resource(MapCollision(StaticWorld::new()))
            .insert_resource(ClearColor(SKY));
    }
}

/// The collision side of the loaded level, read by the movement tick.
#[derive(Resource)]
pub struct MapCollision(pub StaticWorld);

/// Murky dusk: the sky before a level sets its own. Also the distance fog's
/// colour, so far things fade into it.
pub const SKY: Color = Color::srgb(0.24, 0.17, 0.14);

/// Every surface texture repeats every this many units.
const TEXTURE_SPAN: f32 = 128.0;
/// Surface textures are this many pixels square: chunky on purpose.
const TEXELS: u32 = 32;

/// What a box is made of: its colour and texture.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Floor,
    Wall,
    Ramp,
    Step,
    Crate,
    Marker,
    /// Floating platforms and catwalks, for the grappling hook.
    Platform,
    /// Flat concrete roofs.
    Roof,
    /// Corrugated steel: shipping containers, cars. Four paints.
    Container(u8),
    /// Dark cast iron: furnaces, machinery.
    Iron,
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
            Kind::Roof => Color::srgb(0.36, 0.34, 0.33),
            Kind::Container(paint) => match paint % 4 {
                0 => Color::srgb(0.55, 0.2, 0.12),  // rust red
                1 => Color::srgb(0.2, 0.3, 0.45),   // blue
                2 => Color::srgb(0.25, 0.38, 0.25), // green
                _ => Color::srgb(0.62, 0.46, 0.16), // ochre
            },
            Kind::Iron => Color::srgb(0.24, 0.23, 0.23),
        }
    }

    fn pattern(self) -> Pattern {
        match self {
            Kind::Floor | Kind::Platform => Pattern::Slabs,
            Kind::Wall => Pattern::Bricks,
            Kind::Ramp | Kind::Crate => Pattern::Planks,
            Kind::Step | Kind::Roof => Pattern::Concrete,
            Kind::Marker => Pattern::Plain,
            Kind::Container(_) | Kind::Iron => Pattern::Corrugated,
        }
    }
}

/// The grey pattern each kind of surface is textured with.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Pattern {
    /// Big stone floor slabs.
    Slabs,
    Bricks,
    Planks,
    /// Blotchy, cracked concrete.
    Concrete,
    /// Ridged sheet steel.
    Corrugated,
    /// Just grain.
    Plain,
}

/// One box of a level.
pub struct Piece {
    pub center: Vec3,
    pub half: Vec3,
    pub rotation: Quat,
    pub kind: Kind,
}

/// An axis-aligned box from corner `min` to corner `max`.
pub fn block(min: Vec3, max: Vec3, kind: Kind) -> Piece {
    Piece {
        center: (min + max) * 0.5,
        half: (max - min) * 0.5,
        rotation: Quat::IDENTITY,
        kind,
    }
}

/// A ramp `width` wide centred on `x`, rising towards -Z from the floor at `z0`
/// over `run` units at `degrees`, with a platform at the top.
pub fn ramp(pieces: &mut Vec<Piece>, x: f32, z0: f32, run: f32, degrees: f32, width: f32) {
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

/// A sloping slab `width` wide whose top surface runs straight from `low` to
/// `high` (both at the slab's middle, differing only in Y and Z). The low end
/// carries on a little way down, so there's no lip where it meets the ground.
pub fn slope(low: Vec3, high: Vec3, width: f32, kind: Kind) -> Piece {
    const THICKNESS: f32 = 16.0;
    const SUNK: f32 = 32.0;
    let along = (high - low).normalize();
    let low = low - along * SUNK;
    // Turn about X only, keeping the slab's top face up whichever way it rises.
    let rotation = Quat::from_rotation_arc(Vec3::Z, along * along.z.signum());
    let up = rotation * Vec3::Y;
    Piece {
        center: (low + high) * 0.5 - up * (THICKNESS * 0.5),
        half: Vec3::new(width * 0.5, THICKNESS * 0.5, low.distance(high) * 0.5),
        rotation,
        kind,
    }
}

/// `count` steps of `rise` each, 32 deep, `width` wide, climbing towards -Z from
/// `z0`, with a landing at the top.
pub fn stairs(pieces: &mut Vec<Piece>, x: f32, z0: f32, rise: f32, count: usize, width: f32) {
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

pub fn collision(pieces: &[Piece]) -> StaticWorld {
    let mut world = StaticWorld::new();
    for piece in pieces {
        world.add_box(piece.center, piece.half, piece.rotation);
    }
    world
}

/// Spawn the visible side of `pieces`, every colour multiplied by `tint` (each
/// level has its own light). They all belong to the level.
pub fn spawn_pieces(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    pieces: &[Piece],
    tint: Color,
) {
    let tint = tint.to_srgba();
    let mut textures = std::collections::HashMap::new();
    let mut material_for = std::collections::HashMap::new();
    for piece in pieces {
        let material = material_for
            .entry(piece.kind)
            .or_insert_with(|| {
                let pattern = piece.kind.pattern();
                let texture = textures
                    .entry(pattern)
                    .or_insert_with(|| images.add(texture(pattern)))
                    .clone();
                let c = piece.kind.color().to_srgba();
                materials.add(StandardMaterial {
                    base_color: Color::srgb(
                        c.red * tint.red,
                        c.green * tint.green,
                        c.blue * tint.blue,
                    ),
                    base_color_texture: Some(texture),
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
            LevelThing,
            Mesh3d(meshes.add(box_mesh(piece.half))),
            MeshMaterial3d(material),
            Transform::from_translation(piece.center).with_rotation(piece.rotation),
        ));
    }
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
        Pattern::Corrugated => {
            // Ridges four pixels apart, lit on one side; rust spots here and there.
            let ridge = [0.62, 0.84, 0.74, 0.5][(x % 4) as usize];
            let rust = if noise(x / 3, y / 3, 8) > 0.9 {
                0.15
            } else {
                0.0
            };
            ridge + 0.08 * grain - rust
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

    #[test]
    fn a_slope_runs_from_its_low_end_to_its_high_end() {
        // Rising towards +Z and towards -Z.
        for (low, high) in [
            (
                Vec3::new(0.0, -320.0, -1840.0),
                Vec3::new(0.0, 0.0, -1200.0),
            ),
            (
                Vec3::new(0.0, -320.0, -2560.0),
                Vec3::new(0.0, 0.0, -3200.0),
            ),
        ] {
            let piece = slope(low, high, 200.0, Kind::Ramp);
            let up = piece.rotation * Vec3::Y;
            let along = piece.rotation * Vec3::Z;
            // One end of the top face is right at `high`.
            let top = piece.center + up * piece.half.y;
            let end = [1.0, -1.0]
                .map(|s| (top + along * piece.half.z * s).distance(high))
                .into_iter()
                .fold(f32::MAX, f32::min);
            assert!(end < 1e-2, "{end}");
            // Top face up, and walkable: under 45 degrees.
            assert!(up.y > 45f32.to_radians().cos(), "{up}");
        }
    }
}
