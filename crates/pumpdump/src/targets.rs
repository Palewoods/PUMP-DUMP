//! Target dummies to shoot at. They flash when hit, burst into chunks when their
//! health runs out, and come back a few seconds later.
//!
//! Dummies don't block movement: they're not in the map's collision world.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::map::SPAWN;

pub struct TargetsPlugin;

impl Plugin for TargetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<TargetHit>()
            .add_systems(Startup, spawn_targets)
            .add_systems(FixedUpdate, (apply_hits, respawn).chain())
            .add_systems(Update, (flash, fly_chunks));
    }
}

/// Health of a fresh dummy.
const HEALTH: f32 = 100.0;
/// Seconds a dead dummy stays gone.
const RESPAWN_TIME: f32 = 3.0;
/// How long a hit makes a dummy glow, seconds.
const FLASH_TIME: f32 = 0.08;
/// Hitbox half-size: about a person, 88 tall. Its centre is this far above the feet.
const HALF: Vec3 = Vec3::new(14.0, 44.0, 10.0);
const BODY_COLOUR: Color = Color::srgb(0.55, 0.16, 0.12);
/// Chunks a dummy bursts into, and how long they last.
const CHUNKS: usize = 10;
const CHUNK_LIFE: f32 = 1.4;
const CHUNK_GRAVITY: f32 = 1100.0;

/// Feet positions of the dummies around the map.
const PLACES: [Vec3; 7] = [
    Vec3::new(-250.0, 0.0, -500.0),
    Vec3::new(150.0, 0.0, -1150.0),
    Vec3::new(-700.0, 0.0, -1200.0),
    Vec3::new(0.0, 0.0, -2100.0),
    Vec3::new(80.0, 0.0, -2900.0),
    // On the platform at the top of the 30° ramp.
    Vec3::new(650.0, 230.94, -820.0),
    // Behind the spawn.
    Vec3::new(SPAWN.x - 300.0, 0.0, 650.0),
];

/// Damage dealt to a dummy. Written by weapons, applied here.
#[derive(Message, Clone, Copy)]
pub struct TargetHit {
    pub target: Entity,
    pub damage: f32,
    /// Which way the shot was going. Chunks fly that way.
    pub direction: Vec3,
}

#[derive(Component)]
pub struct Target {
    health: f32,
    /// Seconds since it died, while it's dead.
    dead_for: Option<f32>,
    /// Seconds of hit glow left.
    flash: f32,
    material: Handle<StandardMaterial>,
}

impl Target {
    /// Hitbox half-size. The entity's translation is the hitbox centre.
    pub const HALF: Vec3 = HALF;

    pub fn alive(&self) -> bool {
        self.dead_for.is_none()
    }
}

/// A flying chunk of a destroyed dummy.
#[derive(Component)]
struct Chunk {
    velocity: Vec3,
    life: f32,
    /// Height of the ground it bounces on.
    floor: f32,
}

#[derive(Resource)]
struct ChunkLook {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

fn spawn_targets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let body = meshes.add(Cuboid::new(28.0, 68.0, 20.0));
    let head = meshes.add(Cuboid::new(16.0, 16.0, 16.0));
    for feet in PLACES {
        // Each dummy has its own material so it can glow on its own when hit.
        let material = materials.add(StandardMaterial {
            base_color: BODY_COLOUR,
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            ..default()
        });
        commands
            .spawn((
                Target {
                    health: HEALTH,
                    dead_for: None,
                    flash: 0.0,
                    material: material.clone(),
                },
                Transform::from_translation(feet + Vec3::Y * HALF.y),
                Visibility::Visible,
            ))
            .with_children(|dummy| {
                dummy.spawn((
                    Mesh3d(body.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(0.0, 34.0 - HALF.y, 0.0),
                ));
                dummy.spawn((
                    Mesh3d(head.clone()),
                    MeshMaterial3d(material),
                    Transform::from_xyz(0.0, 80.0 - HALF.y, 0.0),
                ));
            });
    }
    commands.insert_resource(ChunkLook {
        mesh: meshes.add(Cuboid::new(6.0, 6.0, 6.0)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.45, 0.1, 0.08),
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            ..default()
        }),
    });
}

fn apply_hits(
    mut commands: Commands,
    mut hits: MessageReader<TargetHit>,
    mut targets: Query<(&mut Target, &mut Visibility, &Transform)>,
    look: Res<ChunkLook>,
) {
    for hit in hits.read() {
        let Ok((mut target, mut visibility, transform)) = targets.get_mut(hit.target) else {
            continue;
        };
        if !target.alive() {
            continue;
        }
        target.health -= hit.damage;
        target.flash = FLASH_TIME;
        if target.health <= 0.0 {
            target.dead_for = Some(0.0);
            *visibility = Visibility::Hidden;
            burst(&mut commands, &look, transform.translation, hit.direction);
        }
    }
}

/// Throw chunks out from `centre`, mostly along the shot and upwards.
fn burst(commands: &mut Commands, look: &ChunkLook, centre: Vec3, direction: Vec3) {
    let push = Vec3::new(direction.x, 0.0, direction.z).normalize_or_zero();
    for k in 0..CHUNKS {
        // Spread the chunks evenly around a circle, each a little different.
        let angle = k as f32 * 2.399; // the golden angle, radians
        let spread = Vec3::new(angle.cos(), 0.0, angle.sin());
        let up = 250.0 + 180.0 * ((k * 7 % 5) as f32 / 4.0);
        let velocity = push * 260.0 + spread * 160.0 + Vec3::Y * up;
        commands.spawn((
            Chunk {
                velocity,
                life: CHUNK_LIFE,
                floor: centre.y - HALF.y,
            },
            Mesh3d(look.mesh.clone()),
            MeshMaterial3d(look.material.clone()),
            Transform::from_translation(centre + spread * 6.0 + Vec3::Y * (k as f32 * 6.0 - 20.0)),
            NotShadowCaster,
        ));
    }
}

fn respawn(time: Res<Time>, mut targets: Query<(&mut Target, &mut Visibility)>) {
    for (mut target, mut visibility) in &mut targets {
        let Some(dead_for) = target.dead_for else {
            continue;
        };
        let dead_for = dead_for + time.delta_secs();
        if dead_for >= RESPAWN_TIME {
            target.dead_for = None;
            target.health = HEALTH;
            *visibility = Visibility::Visible;
        } else {
            target.dead_for = Some(dead_for);
        }
    }
}

/// Glow white for a moment after each hit.
fn flash(
    time: Res<Time>,
    mut targets: Query<&mut Target>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for mut target in &mut targets {
        let was_lit = target.flash > 0.0;
        target.flash = (target.flash - time.delta_secs()).max(0.0);
        let lit = target.flash > 0.0;
        // Only touch the material when the glow turns on or off.
        if (was_lit || lit)
            && let Some(mut material) = materials.get_mut(&target.material)
        {
            material.emissive = if lit {
                LinearRgba::rgb(6.0, 5.0, 4.0)
            } else {
                LinearRgba::BLACK
            };
        }
    }
}

fn fly_chunks(
    mut commands: Commands,
    time: Res<Time>,
    mut chunks: Query<(Entity, &mut Chunk, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut chunk, mut transform) in &mut chunks {
        chunk.life -= dt;
        if chunk.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        chunk.velocity.y -= CHUNK_GRAVITY * dt;
        transform.translation += chunk.velocity * dt;
        // Bounce (badly) off the ground they started on.
        if transform.translation.y < chunk.floor + 3.0 && chunk.velocity.y < 0.0 {
            transform.translation.y = chunk.floor + 3.0;
            chunk.velocity *= Vec3::new(0.5, -0.3, 0.5);
        }
        transform.rotate_x(dt * 9.0);
        // Shrink away over the last third of their life.
        transform.scale = Vec3::splat((chunk.life / (CHUNK_LIFE / 3.0)).min(1.0));
    }
}

/// Distance along a ray (unit `direction`) to where it enters a box, if it does.
/// Zero if the ray starts inside.
pub fn ray_box(origin: Vec3, direction: Vec3, centre: Vec3, half: Vec3) -> Option<f32> {
    // Slab method: the ray is inside the box between where it has crossed into all
    // three pairs of planes and before it has crossed out of any of them.
    let inverse = direction.recip();
    let a = (centre - half - origin) * inverse;
    let b = (centre + half - origin) * inverse;
    let near = a.min(b).max_element();
    let far = a.max(b).min_element();
    (far >= near.max(0.0)).then_some(near.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_box_hits_misses_and_starts_inside() {
        let half = Vec3::splat(10.0);
        let hit = ray_box(Vec3::new(0.0, 0.0, 100.0), Vec3::NEG_Z, Vec3::ZERO, half);
        assert_eq!(hit, Some(90.0));
        // Pointing away.
        assert_eq!(
            ray_box(Vec3::new(0.0, 0.0, 100.0), Vec3::Z, Vec3::ZERO, half),
            None
        );
        // Passing beside it.
        let beside = ray_box(Vec3::new(20.0, 0.0, 100.0), Vec3::NEG_Z, Vec3::ZERO, half);
        assert_eq!(beside, None);
        // Starting inside.
        assert_eq!(ray_box(Vec3::ZERO, Vec3::X, Vec3::ZERO, half), Some(0.0));
        // Axis-aligned rays have infinite inverse components; still fine.
        let diagonal = Vec3::new(1.0, 1.0, 0.0).normalize();
        assert!(ray_box(Vec3::new(-50.0, -50.0, 0.0), diagonal, Vec3::ZERO, half).is_some());
    }
}
