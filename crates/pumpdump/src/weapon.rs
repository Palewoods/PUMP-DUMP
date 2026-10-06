//! The double-barrel shotgun.
//!
//! One barrel per click. After both barrels it breaks open and reloads both
//! (or press reload any time it isn't full). Each shot is a cone of hitscan
//! pellets in a fixed pattern that turns a little every shot: fair, but not
//! identical. Its numbers live in `assets/shotgun.weapon.ron`, hot-reloaded like
//! movement.ron.
//!
//! The gun model is drawn by its own camera on its own render layer, on top of
//! the world, so it never pokes through walls.

use std::f32::consts::PI;

use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use leafwing_input_manager::prelude::*;
use pumpdump_movement::CollisionWorld;
use serde::Deserialize;

use crate::character::{self, SKIN, Style, ZombieKit};
use crate::input::{self, Action};
use crate::map::MapCollision;
use crate::player::{self, PlayerCamera, PlayerStatus, ThirdPerson};
use crate::retro::{RetroScreen, VIEW_MODEL_LAYER};
use crate::sfx::{self, Sounds};
use crate::targets::{Damage, Hitbox, ray_box};

pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<ShotgunAsset>()
            .register_asset_loader(ShotgunLoader)
            .init_resource::<Shotgun>()
            .init_resource::<GunFx>()
            .add_systems(Startup, (load_tuning, make_effect_look, spawn_hud))
            // After Startup, so the player's camera exists to attach the gun to.
            .add_systems(PostStartup, spawn_view_model)
            .add_systems(FixedUpdate, tick_shotgun.after(player::MovementTick))
            .add_systems(Update, (animate_view_model, fade_puffs, update_hud));
    }
}

/// Field of view the gun model is drawn with, degrees. Fixed, so the gun doesn't
/// stretch when the world's FOV widens at speed.
const VIEW_MODEL_FOV: f32 = 60.0;
/// Where the gun sits relative to the eye, units: right, down, forwards (-Z).
const GUN_REST: Vec3 = Vec3::new(7.5, -8.0, -14.0);
/// How fast the recoil kick settles, per second.
const KICK_RECOVERY: f32 = 9.0;
/// Seconds the muzzle flash shows.
const MUZZLE_FLASH_TIME: f32 = 0.05;
/// Brightness of the muzzle flash's light on the surroundings, lumens.
const MUZZLE_LIGHT: f32 = 4.0e9;
/// Seconds the hit marker shows after a pellet hits a target.
const HIT_MARKER_TIME: f32 = 0.12;
/// Seconds an impact puff lasts.
const PUFF_LIFE: f32 = 0.35;

// ---- tuning ----

/// Units: game units (~1 inch), seconds, degrees.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShotgunTuning {
    /// Shells it holds: one per barrel.
    pub barrels: u32,
    /// Pellets per shot.
    pub pellets: u32,
    /// Half-angle of the cone the pellets fill, degrees.
    pub spread_deg: f32,
    /// Damage per pellet. Dummies have 100 health.
    pub pellet_damage: f32,
    /// Pellets stop after this far, units.
    pub range: f32,
    /// Seconds after firing one barrel before the next can fire.
    pub refire_time: f32,
    /// Seconds to reload both barrels.
    pub reload_time: f32,
}

#[derive(Asset, TypePath, Deref)]
pub struct ShotgunAsset(pub ShotgunTuning);

#[derive(Resource)]
struct ShotgunHandle(Handle<ShotgunAsset>);

#[derive(TypePath)]
struct ShotgunLoader;

impl AssetLoader for ShotgunLoader {
    type Asset = ShotgunAsset;
    type Settings = ();
    type Error = Box<dyn std::error::Error + Send + Sync>;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<ShotgunAsset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(ShotgunAsset(ron::de::from_bytes(&bytes)?))
    }

    fn extensions(&self) -> &[&str] {
        &["weapon.ron"]
    }
}

fn load_tuning(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(ShotgunHandle(assets.load("shotgun.weapon.ron")));
}

// ---- rules ----

/// The shotgun's state. Only [`Shotgun::tick`] changes it.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Shotgun {
    /// Shells in the barrels.
    pub loaded: u32,
    /// Seconds before a barrel can fire again.
    cooldown: f32,
    /// Seconds into a reload, while reloading.
    pub reloading: Option<f32>,
    /// Shots fired so far. Turns the pellet pattern each shot.
    shots: u32,
}

impl Default for Shotgun {
    fn default() -> Self {
        Self {
            loaded: 2,
            cooldown: 0.0,
            reloading: None,
            shots: 0,
        }
    }
}

/// What happened during one tick, for sounds and effects.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct GunEvents {
    pub fired: bool,
    /// Broke open to reload.
    pub opened: bool,
    /// Halfway through a reload: shells go in.
    pub shells_in: bool,
    /// Reload finished: snapped shut, full.
    pub closed: bool,
}

impl Shotgun {
    /// Advance `dt` seconds. `fire` and `reload` are presses this tick.
    pub fn tick(&mut self, fire: bool, reload: bool, tuning: &ShotgunTuning, dt: f32) -> GunEvents {
        let mut events = GunEvents::default();
        self.cooldown = (self.cooldown - dt).max(0.0);
        self.loaded = self.loaded.min(tuning.barrels);

        // A reload can't be interrupted.
        if let Some(elapsed) = self.reloading {
            let now = elapsed + dt;
            let half = tuning.reload_time * 0.5;
            events.shells_in = elapsed < half && now >= half;
            if now >= tuning.reload_time {
                self.reloading = None;
                self.loaded = tuning.barrels;
                events.closed = true;
            } else {
                self.reloading = Some(now);
            }
            return events;
        }
        if self.cooldown > 0.0 {
            return events;
        }
        if fire && self.loaded > 0 {
            self.loaded -= 1;
            self.cooldown = tuning.refire_time;
            self.shots += 1;
            events.fired = true;
        } else if self.loaded < tuning.barrels && (reload || self.loaded == 0) {
            // Empty reloads by itself; part-loaded only when asked.
            self.reloading = Some(0.0);
            events.opened = true;
        }
        events
    }
}

/// Directions of a shot's pellets: `count` of them evenly filling a cone of
/// half-angle `spread_deg` around `forward`, turned by `spin` radians.
///
/// The pattern is a sunflower spiral: each pellet is a golden angle round from
/// the last and a little further out, which spreads any number of them evenly.
pub fn pellet_directions(
    forward: Vec3,
    right: Vec3,
    up: Vec3,
    count: u32,
    spread_deg: f32,
    spin: f32,
) -> impl Iterator<Item = Vec3> {
    const GOLDEN_ANGLE: f32 = 2.399_963;
    let reach = spread_deg.to_radians().tan();
    (0..count).map(move |i| {
        // sqrt spaces the rings so each covers the same area.
        let r = reach * ((i as f32 + 0.5) / count as f32).sqrt();
        let (sin, cos) = (i as f32 * GOLDEN_ANGLE + spin).sin_cos();
        (forward + right * (r * cos) + up * (r * sin)).normalize()
    })
}

// ---- simulation ----

/// Effects state the fixed tick sets and the frame-rate systems animate.
#[derive(Resource, Default)]
struct GunFx {
    /// Recoil, 1 just after a shot, easing to 0.
    kick: f32,
    /// Seconds of muzzle flash left.
    flash: f32,
    /// Seconds of hit marker left.
    hit_marker: f32,
}

// Rust note: Bevy systems ask for everything they use as parameters, so busy
// ones have a lot of them. That's normal in Bevy, so the lint is switched off.
#[allow(clippy::too_many_arguments)]
fn tick_shotgun(
    mut commands: Commands,
    time: Res<Time>,
    handle: Res<ShotgunHandle>,
    tunings: Res<Assets<ShotgunAsset>>,
    sounds: Option<Res<Sounds>>,
    look: Res<EffectLook>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    camera: Single<(&Transform, &ActionState<Action>), With<PlayerCamera>>,
    map: Res<MapCollision>,
    targets: Query<(Entity, &Transform, &Hitbox)>,
    mut gun: ResMut<Shotgun>,
    mut fx: ResMut<GunFx>,
    mut hits: MessageWriter<Damage>,
) {
    let Some(tuning) = tunings.get(&handle.0) else {
        return;
    };
    let (eye, actions) = camera.into_inner();
    // The click that captures the mouse shouldn't also fire.
    let fire = input::cursor_captured(&cursor) && actions.just_pressed(&Action::Fire);
    let reload = actions.just_pressed(&Action::Reload);
    let events = gun.tick(fire, reload, tuning, time.delta_secs());

    if let Some(sounds) = &sounds {
        for (happened, sound) in [
            (events.fired, &sounds.shotgun),
            (events.opened, &sounds.open),
            (events.shells_in, &sounds.load),
            (events.closed, &sounds.close),
        ] {
            if happened {
                sfx::play(&mut commands, sound);
            }
        }
    }
    if !events.fired {
        return;
    }
    fx.kick = 1.0;
    fx.flash = MUZZLE_FLASH_TIME;

    let origin = eye.translation;
    let spin = gun.shots as f32 * 2.1;
    let pellets = pellet_directions(
        *eye.forward(),
        *eye.right(),
        *eye.up(),
        tuning.pellets,
        tuning.spread_deg,
        spin,
    );
    let mut hit_target = false;
    for direction in pellets {
        let wall = map
            .0
            .cast_ray(origin, direction * tuning.range)
            .map(|hit| (hit.fraction * tuning.range, hit.normal));
        let target = targets
            .iter()
            .filter(|(_, _, hitbox)| hitbox.enabled)
            .filter_map(|(entity, transform, hitbox)| {
                let distance = ray_box(origin, direction, hitbox.centre(transform), hitbox.half)?;
                (distance <= tuning.range).then_some((distance, entity))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));

        match (wall, target) {
            (_, Some((distance, entity))) if wall.is_none_or(|(w, _)| distance < w) => {
                let point = origin + direction * distance;
                hits.write(Damage {
                    target: entity,
                    amount: tuning.pellet_damage,
                    direction,
                });
                spawn_puff(&mut commands, &look, &look.blood, point - direction * 2.0);
                hit_target = true;
            }
            (Some((distance, normal)), _) => {
                let point = origin + direction * distance + normal * 1.5;
                spawn_puff(&mut commands, &look, &look.dust, point);
            }
            _ => {}
        }
    }
    if hit_target {
        fx.hit_marker = HIT_MARKER_TIME;
        if let Some(sounds) = &sounds {
            sfx::play(&mut commands, &sounds.hit);
        }
    }
}

// ---- impact puffs ----

#[derive(Resource)]
struct EffectLook {
    puff: Handle<Mesh>,
    dust: Handle<StandardMaterial>,
    blood: Handle<StandardMaterial>,
}

/// A little cloud where a pellet hit. Pops up, then shrinks away.
#[derive(Component)]
struct Puff {
    life: f32,
}

fn make_effect_look(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let flat = |color: Color| StandardMaterial {
        base_color: color,
        unlit: true,
        ..default()
    };
    commands.insert_resource(EffectLook {
        puff: meshes.add(Cuboid::new(4.0, 4.0, 4.0)),
        dust: materials.add(flat(Color::srgb(0.55, 0.5, 0.42))),
        blood: materials.add(flat(Color::srgb(0.5, 0.05, 0.04))),
    });
}

fn spawn_puff(
    commands: &mut Commands,
    look: &EffectLook,
    material: &Handle<StandardMaterial>,
    at: Vec3,
) {
    commands.spawn((
        Puff { life: PUFF_LIFE },
        Mesh3d(look.puff.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(at),
        NotShadowCaster,
    ));
}

fn fade_puffs(
    mut commands: Commands,
    time: Res<Time>,
    mut puffs: Query<(Entity, &mut Puff, &mut Transform)>,
) {
    for (entity, mut puff, mut transform) in &mut puffs {
        puff.life -= time.delta_secs();
        if puff.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let age = 1.0 - puff.life / PUFF_LIFE; // 0..1
        transform.scale = Vec3::splat((age * PI).sin() * 1.6 + 0.2);
        transform.translation.y += time.delta_secs() * 12.0;
    }
}

// ---- the gun model ----

/// The gun, moved around by recoil, reloads and walking.
#[derive(Component)]
struct ViewModel;

#[derive(Component)]
struct MuzzleFlash;

/// Lights up the surroundings for a moment on each shot.
#[derive(Component)]
struct FlashLight;

fn spawn_view_model(
    mut commands: Commands,
    eye: Single<Entity, With<PlayerCamera>>,
    screen: Res<RetroScreen>,
    kit: Res<ZombieKit>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let layer = RenderLayers::layer(VIEW_MODEL_LAYER);
    let metal = materials.add(StandardMaterial {
        base_color: Color::srgb(0.17, 0.17, 0.19),
        metallic: 0.7,
        perceptual_roughness: 0.45,
        ..default()
    });
    let wood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.36, 0.19, 0.09),
        perceptual_roughness: 0.85,
        reflectance: 0.1,
        ..default()
    });
    let fire = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.8, 0.35),
        emissive: LinearRgba::rgb(30.0, 18.0, 6.0),
        unlit: true,
        ..default()
    });
    let coat = materials.add(StandardMaterial {
        base_color: Style::PLAYER.coat,
        perceptual_roughness: 1.0,
        reflectance: 0.05,
        ..default()
    });
    let rot = materials.add(StandardMaterial {
        base_color: SKIN.darker(0.15),
        perceptual_roughness: 1.0,
        ..default()
    });
    // The player's rotting hands, gripping the gun, in coat sleeves that run back
    // out of view. All boxes: the kit's unit cube, stretched. Gun-local space.
    let hands = [
        // Right hand round the wrist of the stock, fingers curled under, one on
        // the trigger, a rotten patch on the back.
        (
            &kit.skin(),
            character::slab(Vec3::new(0.3, -2.6, 7.0), Vec3::new(3.4, 3.4, 4.0)),
        ),
        (
            &kit.skin(),
            character::slab(Vec3::new(0.2, -4.2, 6.0), Vec3::new(3.0, 1.2, 2.4)),
        ),
        (
            &kit.skin(),
            character::slab(Vec3::new(0.5, -2.4, 4.0), Vec3::new(0.9, 0.9, 2.4)),
        ),
        (
            &rot,
            character::slab(Vec3::new(0.3, -0.9, 7.2), Vec3::new(2.4, 0.4, 2.4)),
        ),
        (
            &coat,
            character::limb(
                Vec3::new(0.6, -3.0, 8.5),
                Vec3::new(3.5, -9.0, 22.0),
                Vec2::splat(5.2),
            ),
        ),
        // Left hand cupping the fore-end: palm under, thumb along one side,
        // clawed fingers along the other.
        (
            &kit.skin(),
            character::slab(Vec3::new(0.0, -2.7, -9.5), Vec3::new(3.6, 2.2, 5.0)),
        ),
        (
            &kit.skin(),
            character::slab(Vec3::new(-1.8, -1.3, -9.5), Vec3::new(1.0, 1.4, 3.6)),
        ),
        (
            &kit.skin(),
            character::slab(Vec3::new(1.8, -1.4, -9.8), Vec3::new(1.0, 1.6, 4.4)),
        ),
        (
            &rot,
            character::slab(Vec3::new(0.0, -3.9, -9.0), Vec3::new(2.6, 0.4, 3.0)),
        ),
        (
            &coat,
            character::limb(
                Vec3::new(-0.5, -3.6, -7.5),
                Vec3::new(-14.0, -8.0, 9.0),
                Vec2::splat(5.2),
            ),
        ),
    ];
    let barrel = meshes.add(Cylinder::new(0.8, 21.0));
    let along_z = Quat::from_rotation_x(PI / 2.0);
    // (mesh, material, position, rotation) for each part, gun pointing along -Z.
    let parts = [
        (
            barrel.clone(),
            &metal,
            Vec3::new(-0.85, 0.0, -10.5),
            along_z,
        ),
        (barrel, &metal, Vec3::new(0.85, 0.0, -10.5), along_z),
        // Rib along the top, between the barrels.
        (
            meshes.add(Cuboid::new(0.7, 0.4, 20.0)),
            &metal,
            Vec3::new(0.0, 0.75, -10.5),
            Quat::IDENTITY,
        ),
        // Receiver.
        (
            meshes.add(Cuboid::new(3.4, 2.8, 6.5)),
            &metal,
            Vec3::new(0.0, -0.5, 2.5),
            Quat::IDENTITY,
        ),
        // Hammers.
        (
            meshes.add(Cuboid::new(0.6, 1.4, 0.9)),
            &metal,
            Vec3::new(-0.9, 1.2, 4.8),
            Quat::from_rotation_x(-0.4),
        ),
        (
            meshes.add(Cuboid::new(0.6, 1.4, 0.9)),
            &metal,
            Vec3::new(0.9, 1.2, 4.8),
            Quat::from_rotation_x(-0.4),
        ),
        // Wooden fore-end under the barrels, and the stock.
        (
            meshes.add(Cuboid::new(2.6, 1.5, 10.0)),
            &wood,
            Vec3::new(0.0, -1.35, -9.0),
            Quat::IDENTITY,
        ),
        (
            meshes.add(Cuboid::new(2.4, 3.4, 13.0)),
            &wood,
            Vec3::new(0.0, -2.9, 11.5),
            Quat::from_rotation_x(-0.22),
        ),
    ];

    commands.entity(*eye).with_children(|eye| {
        // Draws only the gun's layer, on top of the world camera's picture.
        eye.spawn((
            Camera3d::default(),
            Camera {
                order: 1,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            screen.target(),
            Msaa::Off,
            Projection::Perspective(PerspectiveProjection {
                fov: VIEW_MODEL_FOV.to_radians(),
                near: 0.5,
                far: 200.0,
                ..default()
            }),
            layer.clone(),
        ));
        eye.spawn((
            FlashLight,
            PointLight {
                intensity: 0.0,
                range: 1500.0,
                color: Color::srgb(1.0, 0.7, 0.35),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, -40.0),
            RenderLayers::from_layers(&[0, VIEW_MODEL_LAYER]),
        ));
        eye.spawn((
            ViewModel,
            Transform::from_translation(GUN_REST),
            Visibility::default(),
        ))
        .with_children(|gun| {
            for (mesh, material, position, rotation) in parts {
                gun.spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(position).with_rotation(rotation),
                    layer.clone(),
                    NotShadowCaster,
                ));
            }
            for (material, transform) in hands {
                gun.spawn((
                    Mesh3d(kit.cube()),
                    MeshMaterial3d(material.clone()),
                    transform,
                    layer.clone(),
                    NotShadowCaster,
                ));
            }
            gun.spawn((
                MuzzleFlash,
                Transform::from_xyz(0.0, 0.0, -22.0),
                Visibility::Hidden,
            ))
            .with_children(|flash| {
                // Two crossed slabs make a rough star.
                for (size, angle) in [
                    (Vec3::new(5.0, 2.4, 1.5), 0.3),
                    (Vec3::new(2.4, 5.0, 1.5), -0.3),
                ] {
                    flash.spawn((
                        Mesh3d(meshes.add(Cuboid::from_size(size))),
                        MeshMaterial3d(fire.clone()),
                        Transform::from_rotation(Quat::from_rotation_z(angle)),
                        layer.clone(),
                        NotShadowCaster,
                    ));
                }
            });
        });
    });
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn animate_view_model(
    time: Res<Time>,
    gun: Res<Shotgun>,
    handle: Res<ShotgunHandle>,
    tunings: Res<Assets<ShotgunAsset>>,
    status: Res<PlayerStatus>,
    third_person: Res<ThirdPerson>,
    mut fx: ResMut<GunFx>,
    mut bob_phase: Local<f32>,
    // Both change visibility, so Bevy needs telling they're never the same entity.
    model: Single<(&mut Transform, &mut Visibility), (With<ViewModel>, Without<MuzzleFlash>)>,
    flash: Single<&mut Visibility, (With<MuzzleFlash>, Without<ViewModel>)>,
    light: Single<&mut PointLight, With<FlashLight>>,
) {
    let dt = time.delta_secs();
    fx.kick *= (-KICK_RECOVERY * dt).exp();
    fx.flash = (fx.flash - dt).max(0.0);
    fx.hit_marker = (fx.hit_marker - dt).max(0.0);

    // Reloading tips the gun down and over, and back up: 0 -> 1 -> 0.
    let reload = match (gun.reloading, tunings.get(&handle.0)) {
        (Some(elapsed), Some(tuning)) => (elapsed / tuning.reload_time).clamp(0.0, 1.0),
        _ => 0.0,
    };
    let dip = (reload * PI).sin();

    // Walking bob: a little figure-of-eight that speeds up with you.
    let speed = Vec2::new(status.velocity.x, status.velocity.z).length();
    let sway = if status.on_ground {
        (speed / 480.0).min(1.5)
    } else {
        0.0
    };
    *bob_phase += dt * (5.0 + speed * 0.01);
    let phase = *bob_phase;
    let bob = Vec3::new(phase.sin() * 0.4, -(phase * 2.0).sin().abs() * 0.35, 0.0) * sway;

    let kick = fx.kick;
    let (mut model, mut shown) = model.into_inner();
    *shown = if third_person.0 {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    model.translation = GUN_REST + bob + Vec3::new(0.0, kick * 0.6 - dip * 4.0, kick * 4.0);
    model.rotation = Quat::from_euler(
        EulerRot::XYZ,
        kick * 0.32 - dip * 0.9,
        dip * 0.25,
        dip * 0.6,
    );

    let flashing = fx.flash > 0.0;
    *flash.into_inner() = if flashing {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    light.into_inner().intensity = if flashing { MUZZLE_LIGHT } else { 0.0 };
}

// ---- HUD ----

#[derive(Component)]
struct Crosshair;

#[derive(Component)]
struct AmmoText;

fn spawn_hud(mut commands: Commands) {
    // A full-window box that centres the crosshair.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_child((
            Crosshair,
            Node {
                width: px(4),
                height: px(4),
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
        ));
    commands.spawn((
        AmmoText,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(28.0),
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.85, 0.65)),
        Node {
            position_type: PositionType::Absolute,
            right: px(28),
            bottom: px(22),
            ..default()
        },
    ));
}

fn update_hud(
    gun: Res<Shotgun>,
    fx: Res<GunFx>,
    handle: Res<ShotgunHandle>,
    tunings: Res<Assets<ShotgunAsset>>,
    crosshair: Single<(&mut Node, &mut BackgroundColor), With<Crosshair>>,
    mut ammo: Single<&mut Text, With<AmmoText>>,
) {
    let (mut node, mut colour) = crosshair.into_inner();
    let marked = fx.hit_marker > 0.0;
    let size = if marked { px(10) } else { px(4) };
    node.width = size;
    node.height = size;
    colour.0 = if marked {
        Color::srgb(1.0, 0.2, 0.15)
    } else {
        Color::srgba(1.0, 1.0, 1.0, 0.85)
    };

    let barrels = tunings.get(&handle.0).map_or(2, |t| t.barrels);
    ammo.0 = if gun.reloading.is_some() {
        "RELOADING".to_string()
    } else {
        (0..barrels)
            .map(|i| if i < gun.loaded { "[#]" } else { "[ ]" })
            .collect::<Vec<_>>()
            .join(" ")
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn tuning() -> ShotgunTuning {
        ShotgunTuning {
            barrels: 2,
            pellets: 12,
            spread_deg: 5.0,
            pellet_damage: 10.0,
            range: 5000.0,
            refire_time: 0.2,
            reload_time: 1.0,
        }
    }

    /// Run `seconds` of ticks with no input, collecting what happened.
    fn wait(gun: &mut Shotgun, seconds: f32) -> Vec<GunEvents> {
        let t = tuning();
        (0..(seconds / DT).round() as usize)
            .map(|_| gun.tick(false, false, &t, DT))
            .collect()
    }

    #[test]
    fn shipped_tuning_parses() {
        let t: ShotgunTuning = ron::from_str(include_str!("../assets/shotgun.weapon.ron")).unwrap();
        assert_eq!(t.barrels, 2);
        assert!(t.pellets > 0 && t.reload_time > t.refire_time);
    }

    #[test]
    fn one_barrel_per_click_then_it_reloads_itself() {
        let t = tuning();
        let mut gun = Shotgun::default();
        assert!(gun.tick(true, false, &t, DT).fired);
        assert_eq!(gun.loaded, 1);
        // Clicking again straight away does nothing: the next barrel isn't ready.
        assert!(!gun.tick(true, false, &t, DT).fired);
        wait(&mut gun, 0.2);
        assert!(gun.tick(true, false, &t, DT).fired);
        assert_eq!(gun.loaded, 0);

        // Empty: it breaks open by itself once the barrel cooldown is over...
        let events = wait(&mut gun, 0.25);
        assert_eq!(events.iter().filter(|e| e.opened).count(), 1);
        // ...can't fire while reloading...
        assert!(!gun.tick(true, false, &t, DT).fired);
        // ...and is full again after the reload time.
        let events = wait(&mut gun, 1.0);
        assert_eq!(events.iter().filter(|e| e.shells_in).count(), 1);
        assert_eq!(events.iter().filter(|e| e.closed).count(), 1);
        assert_eq!((gun.loaded, gun.reloading), (2, None));
    }

    #[test]
    fn reload_button_tops_up_but_not_when_full() {
        let t = tuning();
        let mut gun = Shotgun::default();
        assert!(!gun.tick(false, true, &t, DT).opened, "reloaded when full");
        gun.tick(true, false, &t, DT);
        wait(&mut gun, 0.3);
        assert_eq!(gun.reloading, None, "one shell left shouldn't auto-reload");
        assert!(gun.tick(false, true, &t, DT).opened);
        // A tick over the reload time: 60 ticks of 1/60 s add up to a hair under 1.
        wait(&mut gun, 1.0 + DT);
        assert_eq!(gun.loaded, 2);
    }

    #[test]
    fn pellets_fill_the_cone_evenly() {
        let dirs: Vec<Vec3> =
            pellet_directions(Vec3::NEG_Z, Vec3::X, Vec3::Y, 12, 5.0, 0.7).collect();
        assert_eq!(dirs.len(), 12);
        let angles: Vec<f32> = dirs
            .iter()
            .map(|d| d.angle_between(Vec3::NEG_Z).to_degrees())
            .collect();
        assert!(dirs.iter().all(|d| d.is_normalized()));
        assert!(angles.iter().all(|&a| a <= 5.0 + 1e-3), "{angles:?}");
        // Uses the whole cone: some near the middle, some near the edge.
        assert!(angles.iter().any(|&a| a < 1.5) && angles.iter().any(|&a| a > 4.0));
        // No two pellets on top of each other.
        for (i, a) in dirs.iter().enumerate() {
            for b in &dirs[i + 1..] {
                assert!(a.angle_between(*b).to_degrees() > 0.5);
            }
        }
    }
}
