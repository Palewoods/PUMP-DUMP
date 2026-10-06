//! Things you can shoot, and practice dummies.
//!
//! Anything with a [`Hitbox`] can be hit by the shotgun, which sends a [`Damage`]
//! message; whoever owns the entity (the dummies here, the enemies in
//! `enemies.rs`) applies it. The dummies flash when hit, burst into chunks when
//! their health runs out, and come back a few seconds later.
//!
//! Dummies don't block movement: they're not in the map's collision world.
//! Levels place them (see `levels.rs`).

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::levels::LevelThing;

pub struct TargetsPlugin;

impl Plugin for TargetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Damage>()
            .add_systems(Startup, make_chunk_look)
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

/// Something the shotgun can hit: a box `half` in size, centred `offset` from
/// the entity's translation. Switched off while whatever it belongs to is dead.
#[derive(Component)]
pub struct Hitbox {
    pub half: Vec3,
    pub offset: Vec3,
    pub enabled: bool,
}

impl Hitbox {
    pub fn centre(&self, transform: &Transform) -> Vec3 {
        transform.translation + self.offset
    }
}

/// Damage dealt to something with a [`Hitbox`]. Written by weapons, applied by
/// whoever owns the entity.
#[derive(Message, Clone, Copy)]
pub struct Damage {
    pub target: Entity,
    pub amount: f32,
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

/// A flying chunk of something destroyed.
#[derive(Component)]
struct Chunk {
    velocity: Vec3,
    life: f32,
    /// Height of the ground it bounces on.
    floor: f32,
}

/// What the chunks of destroyed things look like.
#[derive(Resource)]
pub struct ChunkLook {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// Stand a practice dummy with its feet at `feet`.
pub fn spawn_dummy(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    feet: Vec3,
) -> Entity {
    let body = meshes.add(Cuboid::new(28.0, 68.0, 20.0));
    let head = meshes.add(Cuboid::new(16.0, 16.0, 16.0));
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
            Hitbox {
                half: HALF,
                offset: Vec3::ZERO,
                enabled: true,
            },
            Transform::from_translation(feet + Vec3::Y * HALF.y),
            Visibility::Visible,
        ))
        .with_children(|dummy| {
            dummy.spawn((
                Mesh3d(body),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(0.0, 34.0 - HALF.y, 0.0),
            ));
            dummy.spawn((
                Mesh3d(head),
                MeshMaterial3d(material),
                Transform::from_xyz(0.0, 80.0 - HALF.y, 0.0),
            ));
        })
        .id()
}

fn make_chunk_look(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
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
    mut hits: MessageReader<Damage>,
    mut targets: Query<(&mut Target, &mut Hitbox, &mut Visibility, &Transform)>,
    look: Res<ChunkLook>,
) {
    for hit in hits.read() {
        // Not a dummy (an enemy, say): someone else handles it.
        let Ok((mut target, mut hitbox, mut visibility, transform)) = targets.get_mut(hit.target)
        else {
            continue;
        };
        if target.dead_for.is_some() {
            continue;
        }
        target.health -= hit.amount;
        target.flash = FLASH_TIME;
        if target.health <= 0.0 {
            target.dead_for = Some(0.0);
            hitbox.enabled = false;
            *visibility = Visibility::Hidden;
            let floor = transform.translation.y - HALF.y;
            burst(
                &mut commands,
                &look,
                transform.translation,
                floor,
                hit.direction,
            );
        }
    }
}

/// Throw chunks out from `centre`, mostly along the shot and upwards. They
/// bounce on a floor at height `floor`.
pub fn burst(commands: &mut Commands, look: &ChunkLook, centre: Vec3, floor: f32, direction: Vec3) {
    let push = Vec3::new(direction.x, 0.0, direction.z).normalize_or_zero();
    for k in 0..CHUNKS {
        // Spread the chunks evenly around a circle, each a little different.
        let angle = k as f32 * 2.399; // the golden angle, radians
        let spread = Vec3::new(angle.cos(), 0.0, angle.sin());
        let up = 250.0 + 180.0 * ((k * 7 % 5) as f32 / 4.0);
        let velocity = push * 260.0 + spread * 160.0 + Vec3::Y * up;
        commands.spawn((
            LevelThing,
            Chunk {
                velocity,
                life: CHUNK_LIFE,
                floor,
            },
            Mesh3d(look.mesh.clone()),
            MeshMaterial3d(look.material.clone()),
            Transform::from_translation(centre + spread * 6.0 + Vec3::Y * (k as f32 * 6.0 - 20.0)),
            NotShadowCaster,
        ));
    }
}

fn respawn(time: Res<Time>, mut targets: Query<(&mut Target, &mut Hitbox, &mut Visibility)>) {
    for (mut target, mut hitbox, mut visibility) in &mut targets {
        let Some(dead_for) = target.dead_for else {
            continue;
        };
        let dead_for = dead_for + time.delta_secs();
        if dead_for >= RESPAWN_TIME {
            target.dead_for = None;
            target.health = HEALTH;
            hitbox.enabled = true;
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
