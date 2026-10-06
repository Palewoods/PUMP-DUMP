//! Enemies: human hunters with rifles, out to put the zombie down.
//!
//! They move with the same movement code as the player (so they climb steps and
//! ramps, and walls stop them), only slower and without the fancy moves. Once
//! one sees you it hunts you: closes in, keeps its distance and strafes, and
//! fires slow glowing slugs you can dodge. Its rifle muzzle glows brighter and
//! brighter just before each shot. Dashing makes you untouchable: shots pass
//! through you mid-dash.
//! Killed ones burst apart and come back at their post a while later.
//!
//! There's no pathfinding yet: they head straight for you (or where they last
//! saw you) and hop when something's in the way.

use std::f32::consts::PI;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use pumpdump_movement::{CollisionWorld, MoveInput, MovementState, MovementTuning, step};

use crate::character::{
    self, CharacterKit, Gait, HUMAN_EYE, HumanLook, HumanStyle, MUZZLE_IDLE, RIFLE_MUZZLE,
};
use crate::health::PlayerHit;
use crate::map::MapCollision;
use crate::player::{MovementTick, PlayerStatus, RefillDash};
use crate::sfx::{self, Sounds};
use crate::targets::{self, ChunkLook, Damage, Hitbox};
use crate::tuning::{Tuning, TuningAsset};

pub struct EnemiesPlugin;

/// An enemy died.
#[derive(Message, Clone, Copy)]
pub struct EnemyKilled;

/// Put every enemy back at its post, alive and unaware (a new run).
#[derive(Message, Clone, Copy)]
pub struct ResetEnemies;

impl Plugin for EnemiesPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<EnemyKilled>()
            .add_message::<ResetEnemies>()
            .add_systems(Startup, (make_shot_look, spawn_enemies))
            .add_systems(
                FixedUpdate,
                (reset, take_damage, think, fly_shots)
                    .chain()
                    .after(MovementTick),
            )
            .add_systems(Update, (place_enemies, show_warnings));
    }
}

const HEALTH: f32 = 120.0;
/// Seconds a killed enemy stays gone.
const RESPAWN_TIME: f32 = 10.0;
/// How far they can see you from (if nothing's in the way), units.
const SIGHT_RANGE: f32 = 3500.0;
/// They stop closing in at this distance...
const KEEP_AWAY: f32 = 900.0;
/// ...and back off if you get closer than this.
const TOO_CLOSE: f32 = 350.0;
const WALK_SPEED: f32 = 280.0;
/// Seconds between shots (each enemy waits a little longer or shorter)...
const ATTACK_COOLDOWN: f32 = 1.8;
/// ...and the warning before each: eyes flare, then it fires.
const WINDUP: f32 = 0.55;
/// Seconds before a strafing enemy changes direction.
const STRAFE_TIME: f32 = 1.6;
/// Their slugs: speed (units/s), damage, size, lifetime (s).
const SHOT_SPEED: f32 = 1300.0;
const SHOT_DAMAGE: f32 = 15.0;
const SHOT_RADIUS: f32 = 7.0;
const SHOT_LIFE: f32 = 4.0;
/// How much they lead a moving target: 0 aims where you are, 1 where you'll be.
const LEAD: f32 = 0.5;
/// Hit glow, seconds.
const FLASH_TIME: f32 = 0.08;
/// Hitbox: the model is 72 tall (about 71 with the cap), about 24 wide.
const HITBOX_HALF: Vec3 = Vec3::new(13.0, 37.0, 10.0);
/// The player's body for being hit: an upright capsule this wide, from the feet
/// up to the eyes. Matches movement.ron's capsule.
const PLAYER_RADIUS: f32 = 16.0;
const PLAYER_HEIGHT: f32 = 72.0;
const PLAYER_EYE: f32 = 60.0;
/// The rifle muzzle's glow at the moment it fires (it builds up to this).
const MUZZLE_HOT: LinearRgba = LinearRgba::rgb(14.0, 5.0, 1.0);

/// Where each enemy stands guard.
const POSTS: [Vec3; 7] = [
    Vec3::new(-600.0, 0.0, -1600.0),
    Vec3::new(420.0, 0.0, -1900.0),
    Vec3::new(0.0, 0.0, -3200.0),
    Vec3::new(-1100.0, 0.0, -2400.0),
    Vec3::new(1500.0, 0.0, -800.0),
    Vec3::new(-1500.0, 0.0, -600.0),
    Vec3::new(900.0, 0.0, 700.0),
];

/// Hunters' looks, handed out in turn: (jacket, skin, hair).
const JACKETS: [Color; 4] = [
    Color::srgb(0.27, 0.29, 0.17), // olive drab
    Color::srgb(0.42, 0.36, 0.24), // tan
    Color::srgb(0.16, 0.19, 0.26), // navy
    Color::srgb(0.3, 0.3, 0.3),    // grey
];
const SKINS: [Color; 3] = [
    Color::srgb(0.86, 0.68, 0.55),
    Color::srgb(0.64, 0.46, 0.32),
    Color::srgb(0.4, 0.27, 0.18),
];
const HAIRS: [Color; 3] = [
    Color::srgb(0.08, 0.06, 0.05),
    Color::srgb(0.3, 0.18, 0.08),
    Color::srgb(0.6, 0.48, 0.25),
];

#[derive(Component)]
pub struct Enemy {
    state: MovementState,
    /// Position at the previous tick, for smooth drawing between ticks.
    previous: Vec3,
    post: Vec3,
    yaw: f32,
    health: f32,
    /// Seconds since it died, while it's dead.
    dead_for: Option<f32>,
    /// Has seen (or been shot by) the player: now it hunts.
    alerted: bool,
    /// Where it last saw the player's feet.
    last_seen: Vec3,
    /// Seconds until it may start its next shot.
    cooldown: f32,
    /// Seconds into the warning before a shot, while winding up.
    windup: Option<f32>,
    strafe: f32,
    strafe_timer: f32,
    /// Seconds it's been pushing forward without getting anywhere.
    stuck: f32,
    flash: f32,
    /// Which post it guards (its place in `POSTS`).
    index: usize,
    /// A small per-enemy number, so they don't all act in lockstep.
    quirk: f32,
    /// What its look was last set to (materials are only rewritten on change):
    /// muzzle glow, and whether the jacket was flashing.
    shown: (LinearRgba, bool),
}

impl Enemy {
    fn new(post: Vec3, index: usize) -> Self {
        let quirk = (index as f32 * 0.618_034).fract();
        Self {
            state: MovementState::new(post),
            previous: post,
            post,
            yaw: quirk * 2.0 * PI,
            health: HEALTH,
            dead_for: None,
            alerted: false,
            last_seen: post,
            cooldown: ATTACK_COOLDOWN * (0.5 + quirk),
            windup: None,
            strafe: if index.is_multiple_of(2) { 1.0 } else { -1.0 },
            strafe_timer: STRAFE_TIME * quirk,
            stuck: 0.0,
            flash: 0.0,
            index,
            quirk,
            shown: (MUZZLE_IDLE, false),
        }
    }
}

/// A slug an enemy fired.
#[derive(Component)]
struct Shot {
    velocity: Vec3,
    life: f32,
}

#[derive(Resource)]
struct ShotLook {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

fn make_shot_look(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(ShotLook {
        mesh: meshes.add(Sphere::new(SHOT_RADIUS)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.45, 0.1),
            emissive: LinearRgba::rgb(12.0, 4.0, 0.8),
            unlit: true,
            ..default()
        }),
    });
}

fn spawn_enemies(
    mut commands: Commands,
    kit: Res<CharacterKit>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (index, post) in POSTS.into_iter().enumerate() {
        let style = HumanStyle {
            jacket: JACKETS[index % JACKETS.len()],
            skin: SKINS[index % SKINS.len()],
            hair: HAIRS[(index / 2) % HAIRS.len()],
            helmet: index % 3 == 1,
        };
        let enemy = Enemy::new(post, index);
        let root = character::spawn_human(
            &mut commands,
            &kit,
            &mut materials,
            style,
            Transform::from_translation(post).with_rotation(Quat::from_rotation_y(enemy.yaw)),
        );
        commands.entity(root).insert((
            enemy,
            Hitbox {
                half: HITBOX_HALF,
                offset: Vec3::Y * HITBOX_HALF.y,
                enabled: true,
            },
        ));
    }
}

/// Movement tuning for enemies: the player's, slowed down and with the fancy
/// moves taken away.
fn enemy_tuning(player: &MovementTuning) -> MovementTuning {
    MovementTuning {
        walk_speed: WALK_SPEED,
        sprint_speed: WALK_SPEED,
        ground_accel: 2500.0,
        ground_friction: 2500.0,
        jump_height: 60.0,
        air_jumps: 0,
        dash_charges: 0.0,
        wall_run_min_speed: f32::INFINITY,
        wall_jumps: Some(0),
        max_speed: 1200.0,
        ..player.clone()
    }
}

fn take_damage(
    mut commands: Commands,
    mut damage: MessageReader<Damage>,
    mut enemies: Query<(&mut Enemy, &mut Hitbox, &mut Visibility)>,
    chunks: Res<ChunkLook>,
    sounds: Option<Res<Sounds>>,
    mut refill: MessageWriter<RefillDash>,
    mut killed: MessageWriter<EnemyKilled>,
) {
    for hit in damage.read() {
        let Ok((mut enemy, mut hitbox, mut visibility)) = enemies.get_mut(hit.target) else {
            continue;
        };
        if enemy.dead_for.is_some() {
            continue;
        }
        enemy.health -= hit.amount;
        enemy.flash = FLASH_TIME;
        // Shooting one from behind still gets its attention.
        enemy.alerted = true;
        if enemy.health <= 0.0 {
            enemy.dead_for = Some(0.0);
            enemy.windup = None;
            hitbox.enabled = false;
            *visibility = Visibility::Hidden;
            let feet = enemy.state.position;
            targets::burst(
                &mut commands,
                &chunks,
                feet + Vec3::Y * 40.0,
                feet.y,
                hit.direction,
            );
            if let Some(sounds) = &sounds {
                sfx::play(&mut commands, &sounds.thud);
            }
            // Every kill hands back a dash charge, to keep you moving.
            refill.write(RefillDash);
            killed.write(EnemyKilled);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn think(
    mut commands: Commands,
    time: Res<Time>,
    tuning: Res<Tuning>,
    tunings: Res<Assets<TuningAsset>>,
    map: Res<MapCollision>,
    player: Res<PlayerStatus>,
    look: Res<ShotLook>,
    sounds: Option<Res<Sounds>>,
    mut enemies: Query<(&mut Enemy, &mut Hitbox, &mut Visibility)>,
) {
    let Some(tuning) = tunings.get(&tuning.0) else {
        return;
    };
    let tuning = enemy_tuning(tuning);
    let dt = time.delta_secs();
    let player_eye = player.position + Vec3::Y * PLAYER_EYE;

    for (mut enemy, mut hitbox, mut visibility) in &mut enemies {
        // Dead: wait, then come back at the post.
        if let Some(dead_for) = enemy.dead_for {
            if dead_for + dt >= RESPAWN_TIME {
                *enemy = Enemy::new(enemy.post, enemy.index);
                hitbox.enabled = true;
                *visibility = Visibility::Inherited;
            } else {
                enemy.dead_for = Some(dead_for + dt);
            }
            continue;
        }

        let feet = enemy.state.position;
        let eye = feet + HUMAN_EYE;
        let to_player = player_eye - eye;
        let distance = to_player.length();
        // Seen if in range and nothing solid is in between.
        let sees = distance < SIGHT_RANGE
            && map
                .0
                .cast_ray(eye, to_player)
                .is_none_or(|hit| hit.fraction > 0.98);
        if sees {
            enemy.alerted = true;
            enemy.last_seen = player.position;
        }

        // Where to go: straight at the player while it can see them, otherwise
        // to where it last saw them. Face that way.
        let mut wish = Vec2::ZERO;
        if enemy.alerted {
            let goal = if sees {
                player.position
            } else {
                enemy.last_seen
            };
            let flat = Vec3::new(goal.x - feet.x, 0.0, goal.z - feet.z);
            if flat.length() > 1.0 {
                enemy.yaw = (-flat.x).atan2(-flat.z);
            }
            enemy.strafe_timer -= dt;
            if enemy.strafe_timer <= 0.0 {
                enemy.strafe = -enemy.strafe;
                enemy.strafe_timer = STRAFE_TIME * (0.7 + 0.6 * enemy.quirk);
            }
            wish = if !sees {
                if flat.length() > 60.0 {
                    Vec2::Y
                } else {
                    Vec2::ZERO
                }
            } else if distance > KEEP_AWAY {
                Vec2::Y
            } else if distance < TOO_CLOSE {
                Vec2::NEG_Y
            } else {
                Vec2::new(enemy.strafe, 0.0)
            };
        }

        // Pushing forward but not getting anywhere: hop.
        let flat_speed = Vec2::new(enemy.state.velocity.x, enemy.state.velocity.z).length();
        let mut jump = false;
        if wish.y > 0.0 && enemy.state.on_ground && flat_speed < 40.0 {
            enemy.stuck += dt;
            if enemy.stuck > 0.4 {
                jump = true;
                enemy.stuck = 0.0;
            }
        } else {
            enemy.stuck = 0.0;
        }

        let input = MoveInput {
            wish,
            yaw: enemy.yaw,
            jump,
            ..default()
        };
        enemy.previous = feet;
        enemy.state = step(&enemy.state, &input, &tuning, &map.0, dt);
        if enemy.state.position.y < -2000.0 {
            let post = enemy.post;
            enemy.state = MovementState::new(post);
            enemy.previous = post;
        }

        // Shooting: cool down, wind up (muzzle glows), fire if still in sight.
        if let Some(windup) = enemy.windup {
            if windup + dt >= WINDUP {
                enemy.windup = None;
                enemy.cooldown = ATTACK_COOLDOWN * (0.8 + 0.4 * enemy.quirk);
                if sees {
                    let facing = Quat::from_rotation_y(enemy.yaw);
                    let muzzle = enemy.state.position + facing * RIFLE_MUZZLE;
                    let chest = player.position + Vec3::Y * (PLAYER_HEIGHT * 0.5);
                    let flight = muzzle.distance(chest) / SHOT_SPEED;
                    let aim = chest + player.velocity * flight * LEAD;
                    commands.spawn((
                        Shot {
                            velocity: (aim - muzzle).normalize_or_zero() * SHOT_SPEED,
                            life: SHOT_LIFE,
                        },
                        Mesh3d(look.mesh.clone()),
                        MeshMaterial3d(look.material.clone()),
                        Transform::from_translation(muzzle),
                        NotShadowCaster,
                    ));
                    if let Some(sounds) = &sounds {
                        sfx::play(&mut commands, &sounds.zap);
                    }
                }
            } else {
                enemy.windup = Some(windup + dt);
            }
        } else if sees {
            enemy.cooldown -= dt;
            if enemy.cooldown <= 0.0 {
                enemy.windup = Some(0.0);
            }
        }
    }
}

/// A new run: everyone back at their post, and no shots left in the air.
fn reset(
    mut commands: Commands,
    mut resets: MessageReader<ResetEnemies>,
    mut enemies: Query<(&mut Enemy, &mut Hitbox, &mut Visibility)>,
    shots: Query<Entity, With<Shot>>,
) {
    if resets.read().count() == 0 {
        return;
    }
    for (mut enemy, mut hitbox, mut visibility) in &mut enemies {
        *enemy = Enemy::new(enemy.post, enemy.index);
        hitbox.enabled = true;
        *visibility = Visibility::Inherited;
    }
    for shot in &shots {
        commands.entity(shot).despawn();
    }
}

fn fly_shots(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<MapCollision>,
    player: Res<PlayerStatus>,
    mut shots: Query<(Entity, &mut Shot, &mut Transform)>,
    mut hits: MessageWriter<PlayerHit>,
) {
    let dt = time.delta_secs();
    // The player's body as a line from just above the feet to just below the
    // top of the head; a shot hits if it comes within the body's radius of it.
    let low = player.position + Vec3::Y * PLAYER_RADIUS;
    let high = player.position + Vec3::Y * (PLAYER_HEIGHT - PLAYER_RADIUS);
    for (entity, mut shot, mut transform) in &mut shots {
        shot.life -= dt;
        let travel = shot.velocity * dt;
        let from = transform.translation;
        // Check the middle and the end of this tick's flight.
        let hit_player = [0.5, 1.0].into_iter().any(|t| {
            distance_to_segment(from + travel * t, low, high) < PLAYER_RADIUS + SHOT_RADIUS
        });
        // Mid-dash you're untouchable: shots pass straight through.
        if hit_player && !player.dashing {
            hits.write(PlayerHit {
                damage: SHOT_DAMAGE,
            });
            commands.entity(entity).despawn();
            continue;
        }
        if shot.life <= 0.0 || map.0.cast_ray(from, travel).is_some() {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation += travel;
    }
}

/// Shortest distance from `point` to the line segment from `a` to `b`.
fn distance_to_segment(point: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = ((point - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    point.distance(a + ab * t)
}

/// Draw each enemy between its last two ticks, facing where it's heading.
fn place_enemies(fixed: Res<Time<Fixed>>, mut enemies: Query<(&Enemy, &mut Transform, &mut Gait)>) {
    let blend = fixed.overstep_fraction();
    for (enemy, mut transform, mut gait) in &mut enemies {
        transform.translation = enemy.previous.lerp(enemy.state.position, blend);
        let turn = Quat::from_rotation_y(enemy.yaw);
        transform.rotation = transform.rotation.slerp(turn, 0.35);
        let velocity = enemy.state.velocity;
        gait.speed = Vec2::new(velocity.x, velocity.z).length();
        gait.grounded = enemy.state.on_ground;
    }
}

/// The warning before a shot (the rifle muzzle glows brighter and brighter),
/// and a white flash of the jacket when hit.
fn show_warnings(
    time: Res<Time>,
    mut enemies: Query<(&mut Enemy, &HumanLook)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (mut enemy, look) in &mut enemies {
        enemy.flash = (enemy.flash - time.delta_secs()).max(0.0);
        let glow = match enemy.windup {
            Some(windup) => MUZZLE_IDLE.mix(&MUZZLE_HOT, windup / WINDUP),
            None => MUZZLE_IDLE,
        };
        let flashing = enemy.flash > 0.0;
        if glow != enemy.shown.0
            && let Some(mut material) = materials.get_mut(&look.muzzle_glow)
        {
            material.base_color = Color::LinearRgba(glow);
        }
        if flashing != enemy.shown.1
            && let Some(mut material) = materials.get_mut(&look.jacket)
        {
            material.emissive = if flashing {
                LinearRgba::rgb(5.0, 5.0, 5.0)
            } else {
                LinearRgba::BLACK
            };
        }
        enemy.shown = (glow, flashing);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_to_segment_measures_to_the_nearest_point() {
        let (a, b) = (Vec3::ZERO, Vec3::new(0.0, 10.0, 0.0));
        assert_eq!(distance_to_segment(Vec3::new(3.0, 5.0, 0.0), a, b), 3.0);
        // Beyond the ends, it's the distance to the end.
        assert_eq!(distance_to_segment(Vec3::new(0.0, 14.0, 0.0), a, b), 4.0);
        assert_eq!(distance_to_segment(Vec3::new(0.0, -2.0, 0.0), a, b), 2.0);
    }

    #[test]
    fn enemies_move_with_the_movement_code_but_slower() {
        let player: MovementTuning = ron::from_str(include_str!("../assets/movement.ron")).unwrap();
        let tuning = enemy_tuning(&player);
        let mut world = pumpdump_movement::StaticWorld::new();
        world.add_box(
            Vec3::new(0.0, -10.0, 0.0),
            Vec3::new(5000.0, 10.0, 5000.0),
            Quat::IDENTITY,
        );
        let forward = MoveInput {
            wish: Vec2::Y,
            ..default()
        };
        let mut state = MovementState::new(Vec3::new(0.0, 1.0, 0.0));
        for _ in 0..120 {
            state = step(&state, &forward, &tuning, &world, 1.0 / 60.0);
        }
        let speed = Vec2::new(state.velocity.x, state.velocity.z).length();
        assert!((speed - WALK_SPEED).abs() < 1.0, "{state:?}");
        assert!(state.on_ground);
    }
}
