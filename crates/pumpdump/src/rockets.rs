//! Rockets from the rocket launcher, and their explosions.
//!
//! A rocket flies straight (or, seeking, turns towards a target ahead of it)
//! until it hits something (a wall, or anything with a hitbox) or runs out of
//! fuel, then explodes: whatever it hit takes the direct
//! damage, and everything in the blast takes splash damage that falls off with
//! distance. The blast also throws the player away from it (but never hurts
//! them), so a rocket at your feet launches you: a rocket jump.

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use pumpdump_movement::CollisionWorld;

use crate::levels::LevelThing;
use crate::map::MapCollision;
use crate::player::{Knockback, MovementTick, PlayerStatus};
use crate::retro::VIEW_MODEL_LAYER;
use crate::sfx::{self, Sounds};
use crate::targets::{Damage, Hitbox, ray_box};
use crate::weapon::LauncherTuning;

pub struct RocketsPlugin;

impl Plugin for RocketsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_look)
            .add_systems(FixedUpdate, fly_rockets.after(MovementTick))
            .add_systems(Update, (animate_blasts, fade_smoke));
    }
}

/// Seconds a rocket flies before blowing up on its own.
const FUEL: f32 = 4.0;
/// Seconds an explosion lasts.
const BLAST_TIME: f32 = 0.4;
/// Seconds a puff of rocket smoke lasts.
const SMOKE_TIME: f32 = 0.5;
/// The smoke trail starts this far into the flight, so it isn't in your face.
const SMOKE_AFTER: f32 = 0.06;
/// The player's middle, above the feet, for working out the blast's shove.
const PLAYER_MIDDLE: f32 = 36.0;
/// Blasts throw you up a bit more than straight away from them, which makes
/// rocket jumps launch you rather than skid you along the floor.
const UPWARD_BIAS: f32 = 0.35;
/// Brightness of an explosion's light, lumens.
const BLAST_LIGHT: f32 = 6.0e9;

#[derive(Resource)]
pub struct RocketLook {
    body: Handle<Mesh>,
    casing: Handle<StandardMaterial>,
    flame: Handle<StandardMaterial>,
    ball: Handle<Mesh>,
    fire: Handle<StandardMaterial>,
    core: Handle<StandardMaterial>,
    smoke: Handle<StandardMaterial>,
}

#[derive(Component)]
struct Rocket {
    velocity: Vec3,
    fuel: f32,
    direct: f32,
    splash: f32,
    radius: f32,
    knockback: f32,
    seek: Option<Seek>,
}

/// A rocket that turns towards targets: any within `cone` radians of where
/// it's heading (and not behind a wall), at up to `turn` radians a second.
#[derive(Clone, Copy, Debug)]
pub struct Seek {
    pub cone: f32,
    pub turn: f32,
}

/// `velocity` turned towards the unit direction `towards`, by at most
/// `max_turn` radians. Same speed.
pub fn steer(velocity: Vec3, towards: Vec3, max_turn: f32) -> Vec3 {
    let heading = velocity.normalize_or_zero();
    let angle = heading.angle_between(towards);
    if angle < 1e-5 || heading == Vec3::ZERO {
        return velocity;
    }
    let share = (max_turn / angle).min(1.0);
    Quat::IDENTITY.slerp(Quat::from_rotation_arc(heading, towards), share) * velocity
}

/// An explosion's fireball: swells, then shrinks away, with a flash of light.
#[derive(Component)]
struct Blast {
    age: f32,
    radius: f32,
}

#[derive(Component)]
struct Smoke {
    life: f32,
}

fn make_look(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let glow = |color: Color| StandardMaterial {
        base_color: color,
        unlit: true,
        ..default()
    };
    commands.insert_resource(RocketLook {
        body: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        casing: materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.33, 0.25),
            perceptual_roughness: 0.8,
            ..default()
        }),
        flame: materials.add(glow(Color::srgb(1.0, 0.75, 0.3))),
        ball: meshes.add(Sphere::new(1.0)),
        fire: materials.add(glow(Color::srgb(1.0, 0.55, 0.15))),
        core: materials.add(glow(Color::srgb(1.0, 0.92, 0.6))),
        smoke: materials.add(glow(Color::srgb(0.4, 0.37, 0.33))),
    });
}

/// Fire a rocket from `from` along `direction` (a unit vector), seeking
/// targets if `seek` says so.
pub fn launch(
    commands: &mut Commands,
    look: &RocketLook,
    from: Vec3,
    direction: Vec3,
    tuning: &LauncherTuning,
    seek: Option<Seek>,
) {
    commands
        .spawn((
            LevelThing,
            Rocket {
                velocity: direction * tuning.rocket_speed,
                fuel: FUEL,
                direct: tuning.direct_damage,
                splash: tuning.splash_damage,
                radius: tuning.splash_radius,
                knockback: tuning.knockback,
                seek,
            },
            Transform::from_translation(from).looking_to(direction, Vec3::Y),
            Visibility::default(),
        ))
        .with_children(|rocket| {
            // Casing along -Z (its nose), a flame at the tail.
            rocket.spawn((
                Mesh3d(look.body.clone()),
                MeshMaterial3d(look.casing.clone()),
                Transform::from_xyz(0.0, 0.0, -2.0).with_scale(Vec3::new(2.4, 2.4, 9.0)),
                NotShadowCaster,
            ));
            rocket.spawn((
                Mesh3d(look.body.clone()),
                MeshMaterial3d(look.flame.clone()),
                Transform::from_xyz(0.0, 0.0, 3.5).with_scale(Vec3::new(1.8, 1.8, 3.0)),
                NotShadowCaster,
            ));
        });
}

#[allow(clippy::too_many_arguments)]
fn fly_rockets(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<MapCollision>,
    look: Res<RocketLook>,
    sounds: Option<Res<Sounds>>,
    player: Res<PlayerStatus>,
    // Rockets and targets both have transforms; Bevy needs telling they're
    // never the same entity.
    mut rockets: Query<(Entity, &mut Rocket, &mut Transform), Without<Hitbox>>,
    targets: Query<(Entity, &Transform, &Hitbox), Without<Rocket>>,
    mut hits: MessageWriter<Damage>,
    mut shoves: MessageWriter<Knockback>,
) {
    let dt = time.delta_secs();
    for (entity, mut rocket, mut transform) in &mut rockets {
        let from = transform.translation;
        if let Some(seek) = rocket.seek {
            let heading = rocket.velocity.normalize_or_zero();
            let quarry = targets
                .iter()
                .filter(|(_, _, hitbox)| hitbox.enabled)
                .filter_map(|(_, target, hitbox)| {
                    let to = hitbox.centre(target) - from;
                    let towards = to.normalize_or_zero();
                    let angle = heading.angle_between(towards);
                    let seen = map
                        .0
                        .cast_ray(from, to)
                        .is_none_or(|hit| hit.fraction > 0.98);
                    (angle < seek.cone && seen).then_some((angle, towards))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, towards)) = quarry {
                rocket.velocity = steer(rocket.velocity, towards, seek.turn * dt);
                transform.look_to(rocket.velocity, Vec3::Y);
            }
        }
        let travel = rocket.velocity * dt;
        let length = travel.length();
        let direction = travel / length.max(1e-6);

        let wall = map
            .0
            .cast_ray(from, travel)
            .map(|hit| hit.fraction * length);
        let target = targets
            .iter()
            .filter(|(_, _, hitbox)| hitbox.enabled)
            .filter_map(|(target, transform, hitbox)| {
                let distance = ray_box(from, direction, hitbox.centre(transform), hitbox.half)?;
                (distance <= length).then_some((distance, target))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));

        rocket.fuel -= dt;
        let impact = match (wall, target) {
            (_, Some((distance, struck))) if wall.is_none_or(|w| distance < w) => {
                hits.write(Damage {
                    target: struck,
                    amount: rocket.direct,
                    direction,
                });
                Some(from + direction * distance)
            }
            (Some(distance), _) => Some(from + direction * distance),
            _ if rocket.fuel <= 0.0 => Some(from),
            _ => None,
        };
        let Some(point) = impact else {
            transform.translation += travel;
            if FUEL - rocket.fuel < SMOKE_AFTER {
                continue;
            }
            // A puff of smoke left behind every tick makes a trail.
            commands.spawn((
                Smoke { life: SMOKE_TIME },
                Mesh3d(look.body.clone()),
                MeshMaterial3d(look.smoke.clone()),
                Transform::from_translation(from).with_scale(Vec3::splat(3.0)),
                NotShadowCaster,
            ));
            continue;
        };
        commands.entity(entity).despawn();
        explode(
            &mut commands,
            &look,
            point,
            &rocket,
            &player,
            &targets,
            &mut hits,
            &mut shoves,
        );
        if let Some(sounds) = &sounds {
            sfx::play(&mut commands, &sounds.boom);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn explode(
    commands: &mut Commands,
    look: &RocketLook,
    point: Vec3,
    rocket: &Rocket,
    player: &PlayerStatus,
    targets: &Query<(Entity, &Transform, &Hitbox), Without<Rocket>>,
    hits: &mut MessageWriter<Damage>,
    shoves: &mut MessageWriter<Knockback>,
) {
    // Splash: full at the centre, nothing at the edge, measured to the nearest
    // point of each hitbox.
    for (target, transform, hitbox) in targets {
        if !hitbox.enabled {
            continue;
        }
        let centre = hitbox.centre(transform);
        let nearest = point.clamp(centre - hitbox.half, centre + hitbox.half);
        let falloff = 1.0 - point.distance(nearest) / rocket.radius;
        if falloff > 0.0 {
            hits.write(Damage {
                target,
                amount: rocket.splash * falloff,
                direction: (centre - point).normalize_or(Vec3::Y),
            });
        }
    }

    // The shove: away from the blast and a bit upwards, strongest close in.
    let middle = player.position + Vec3::Y * PLAYER_MIDDLE;
    let distance = point.distance(middle);
    if distance < rocket.radius {
        let away = (middle - point).normalize_or(Vec3::Y);
        let push = (away + Vec3::Y * UPWARD_BIAS).normalize();
        let strength = rocket.knockback * (1.0 - distance / rocket.radius).max(0.3);
        shoves.write(Knockback(push * strength));
    }

    commands
        .spawn((
            Blast {
                age: 0.0,
                radius: rocket.radius,
            },
            Mesh3d(look.ball.clone()),
            MeshMaterial3d(look.fire.clone()),
            Transform::from_translation(point).with_scale(Vec3::splat(1.0)),
            NotShadowCaster,
        ))
        .with_children(|blast| {
            // A hotter, paler core inside the fireball.
            blast.spawn((
                Mesh3d(look.ball.clone()),
                MeshMaterial3d(look.core.clone()),
                Transform::from_scale(Vec3::splat(0.6)),
                NotShadowCaster,
            ));
            blast.spawn((
                PointLight {
                    intensity: BLAST_LIGHT,
                    range: rocket.radius * 6.0,
                    color: Color::srgb(1.0, 0.6, 0.25),
                    ..default()
                },
                RenderLayers::from_layers(&[0, VIEW_MODEL_LAYER]),
            ));
        });
}

fn animate_blasts(
    mut commands: Commands,
    time: Res<Time>,
    mut blasts: Query<(Entity, &mut Blast, &mut Transform, &Children)>,
    mut lights: Query<&mut PointLight>,
) {
    for (entity, mut blast, mut transform, children) in &mut blasts {
        blast.age += time.delta_secs();
        let t = blast.age / BLAST_TIME;
        if t >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        // Swells fast to most of the blast radius, then shrinks away.
        let size = if t < 0.3 {
            t / 0.3
        } else {
            1.0 - (t - 0.3) / 0.7
        };
        transform.scale = Vec3::splat(blast.radius * 0.6 * size.max(0.01));
        for child in children {
            if let Ok(mut light) = lights.get_mut(*child) {
                light.intensity = BLAST_LIGHT * (1.0 - t);
            }
        }
    }
}

fn fade_smoke(
    mut commands: Commands,
    time: Res<Time>,
    mut smoke: Query<(Entity, &mut Smoke, &mut Transform)>,
) {
    for (entity, mut puff, mut transform) in &mut smoke {
        puff.life -= time.delta_secs();
        if puff.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        transform.scale = Vec3::splat(3.0 + (1.0 - puff.life / SMOKE_TIME) * 6.0);
        transform.translation.y += time.delta_secs() * 20.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steering_turns_at_most_so_far_and_keeps_the_speed() {
        let velocity = Vec3::new(0.0, 0.0, -2000.0);
        let right = Vec3::X;
        let turned = steer(velocity, right, 0.1);
        assert!((turned.length() - 2000.0).abs() < 1e-2);
        assert!((turned.angle_between(velocity) - 0.1).abs() < 1e-4);
        // Close enough: straight at it.
        let near = Vec3::new(0.05, 0.0, -1.0).normalize();
        assert!(steer(velocity, near, 0.5).normalize().angle_between(near) < 1e-4);
    }
}
