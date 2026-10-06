//! The player's weapons: a machete, a revolver, the double-barrel shotgun, a
//! tommy gun and a rocket launcher.
//!
//! 1-5 (or the mouse wheel) picks one; F slashes with the machete whatever is
//! out. Guns fire one shot per click, or as long as the trigger is held for the
//! tommy gun, and reload by themselves when empty (or on R). The revolver,
//! shotgun and tommy gun are hitscan: instant rays, the shotgun's spread in a
//! fixed pattern that turns a little each shot. The rocket launcher fires real
//! rockets (see `rockets.rs`).
//!
//! Every number lives in `assets/weapons.ron`, hot-reloaded like movement.ron.
//! The first-person models are in `viewmodel.rs`.

use std::f32::consts::PI;

use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use leafwing_input_manager::prelude::*;
use pumpdump_movement::CollisionWorld;
use serde::Deserialize;

use crate::character;
use crate::input::{self, Action};
use crate::map::MapCollision;
use crate::player::{self, PlayerCamera};
use crate::rockets::{self, RocketLook};
use crate::sfx::{self, Sounds};
use crate::targets::{Damage, Hitbox, ray_box};

pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<WeaponsAsset>()
            .register_asset_loader(WeaponsLoader)
            .init_resource::<Arsenal>()
            .init_resource::<GunFx>()
            .add_systems(Startup, (load_tuning, make_effect_look, spawn_hud))
            .add_systems(FixedUpdate, tick_weapons.after(player::MovementTick))
            .add_systems(Update, (fade_puffs, fade_tracers, update_hud));
    }
}

/// Seconds to bring a weapon up after switching; it can't fire until it's up.
pub const SWITCH_TIME: f32 = 0.25;
/// How long a machete swing takes to play, seconds.
pub const SWING_TIME: f32 = 0.28;
/// Seconds the muzzle flash shows.
const MUZZLE_FLASH_TIME: f32 = 0.05;
/// Seconds the hit marker shows after hitting something.
const HIT_MARKER_TIME: f32 = 0.12;
/// Seconds an impact puff lasts, and a tracer.
const PUFF_LIFE: f32 = 0.35;
const TRACER_LIFE: f32 = 0.06;
/// Where tracers start, relative to the eye: about where the muzzle is.
const TRACER_START: Vec3 = Vec3::new(6.0, -5.0, -28.0);

// ---- tuning ----

/// Every weapon's numbers. Units: game units (~1 inch), seconds, degrees.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponsTuning {
    pub machete: MeleeTuning,
    pub revolver: GunTuning,
    pub shotgun: GunTuning,
    pub tommy_gun: GunTuning,
    pub launcher: LauncherTuning,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeleeTuning {
    pub damage: f32,
    /// How far it reaches, units.
    pub reach: f32,
    /// How wide the slash sweeps, degrees.
    pub arc_deg: f32,
    /// Seconds between swings.
    pub interval: f32,
}

/// A hitscan gun.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunTuning {
    /// Rounds per reload.
    pub magazine: u32,
    /// Fires as long as the trigger is held (otherwise once per click).
    pub automatic: bool,
    /// Seconds between shots.
    pub interval: f32,
    /// Seconds to reload the whole magazine.
    pub reload_time: f32,
    /// Pellets per shot, evenly filling a cone of `spread_deg` half-angle.
    pub pellets: u32,
    pub spread_deg: f32,
    /// Damage per pellet.
    pub damage: f32,
    pub range: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LauncherTuning {
    pub magazine: u32,
    pub interval: f32,
    pub reload_time: f32,
    /// Rocket speed, units/s.
    pub rocket_speed: f32,
    /// Damage to whatever the rocket hits...
    pub direct_damage: f32,
    /// ...and to everything in the blast, falling off to nothing at its edge.
    pub splash_damage: f32,
    pub splash_radius: f32,
    /// How hard the blast shoves you, units/s, at its centre. Rocket jumps!
    pub knockback: f32,
}

impl WeaponsTuning {
    /// The magazine, fire rate and reload of a gun (`None` for the machete).
    pub fn rules(&self, kind: WeaponKind) -> Option<GunRules> {
        let gun = |g: &GunTuning| GunRules {
            magazine: g.magazine,
            interval: g.interval,
            reload_time: g.reload_time,
        };
        match kind {
            WeaponKind::Machete => None,
            WeaponKind::Revolver => Some(gun(&self.revolver)),
            WeaponKind::Shotgun => Some(gun(&self.shotgun)),
            WeaponKind::TommyGun => Some(gun(&self.tommy_gun)),
            WeaponKind::Launcher => Some(GunRules {
                magazine: self.launcher.magazine,
                interval: self.launcher.interval,
                reload_time: self.launcher.reload_time,
            }),
        }
    }

    fn hitscan(&self, kind: WeaponKind) -> Option<&GunTuning> {
        match kind {
            WeaponKind::Revolver => Some(&self.revolver),
            WeaponKind::Shotgun => Some(&self.shotgun),
            WeaponKind::TommyGun => Some(&self.tommy_gun),
            _ => None,
        }
    }
}

#[derive(Asset, TypePath, Deref)]
pub struct WeaponsAsset(pub WeaponsTuning);

#[derive(Resource)]
pub struct WeaponsHandle(pub Handle<WeaponsAsset>);

#[derive(TypePath)]
struct WeaponsLoader;

impl AssetLoader for WeaponsLoader {
    type Asset = WeaponsAsset;
    type Settings = ();
    type Error = Box<dyn std::error::Error + Send + Sync>;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<WeaponsAsset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(WeaponsAsset(ron::de::from_bytes(&bytes)?))
    }

    fn extensions(&self) -> &[&str] {
        &["weapons.ron"]
    }
}

fn load_tuning(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(WeaponsHandle(assets.load("weapons.ron")));
}

// ---- rules ----

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WeaponKind {
    Machete,
    Revolver,
    #[default]
    Shotgun,
    TommyGun,
    Launcher,
}

impl WeaponKind {
    /// In slot order: 1 to 5.
    pub const ALL: [WeaponKind; 5] = [
        WeaponKind::Machete,
        WeaponKind::Revolver,
        WeaponKind::Shotgun,
        WeaponKind::TommyGun,
        WeaponKind::Launcher,
    ];

    /// 0 to 4.
    pub fn slot(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            WeaponKind::Machete => "MACHETE",
            WeaponKind::Revolver => "REVOLVER",
            WeaponKind::Shotgun => "SHOTGUN",
            WeaponKind::TommyGun => "TOMMY GUN",
            WeaponKind::Launcher => "ROCKETS",
        }
    }

    /// The next (or with `step = -1`, previous) weapon, wrapping round.
    pub fn step(self, step: i32) -> WeaponKind {
        let n = Self::ALL.len() as i32;
        Self::ALL[(self.slot() as i32 + step).rem_euclid(n) as usize]
    }
}

/// A gun's magazine, fire rate and reload time.
#[derive(Debug, Clone, Copy)]
pub struct GunRules {
    pub magazine: u32,
    pub interval: f32,
    pub reload_time: f32,
}

/// One gun's state. Only [`Gun::tick`] changes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Gun {
    /// Rounds loaded.
    pub loaded: u32,
    /// Seconds before it can fire again.
    cooldown: f32,
    /// Seconds into a reload, while reloading.
    pub reloading: Option<f32>,
}

impl Default for Gun {
    /// Full: trimmed to the magazine size on the first tick.
    fn default() -> Self {
        Self {
            loaded: u32::MAX,
            cooldown: 0.0,
            reloading: None,
        }
    }
}

/// What happened during one tick, for sounds and effects.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct GunEvents {
    pub fired: bool,
    pub reload_started: bool,
    /// Halfway through a reload.
    pub reload_half: bool,
    pub reload_done: bool,
}

impl Gun {
    /// Advance `dt` seconds. `trigger` asks for a shot this tick; `reload` is a
    /// press of the reload button.
    pub fn tick(&mut self, trigger: bool, reload: bool, rules: &GunRules, dt: f32) -> GunEvents {
        let mut events = GunEvents::default();
        // The cooldown may dip just below zero: that bit of leftover time counts
        // towards the next shot, so fire rates come out exact rather than rounded
        // up to whole ticks. Never more than a tick's worth, so idle time isn't
        // banked.
        self.cooldown = (self.cooldown - dt).max(-dt);
        self.loaded = self.loaded.min(rules.magazine);

        // A reload can't be interrupted (except by switching weapons).
        if let Some(elapsed) = self.reloading {
            let now = elapsed + dt;
            let half = rules.reload_time * 0.5;
            events.reload_half = elapsed < half && now >= half;
            if now >= rules.reload_time {
                self.reloading = None;
                self.loaded = rules.magazine;
                events.reload_done = true;
            } else {
                self.reloading = Some(now);
            }
            return events;
        }
        // (A hair over zero still counts as ready: tick times don't add up exactly.)
        if self.cooldown > 1e-4 {
            return events;
        }
        if trigger && self.loaded > 0 {
            self.loaded -= 1;
            self.cooldown += rules.interval;
            events.fired = true;
        } else if self.loaded < rules.magazine && (reload || self.loaded == 0) {
            // Empty reloads by itself; part-loaded only when asked.
            self.reloading = Some(0.0);
            events.reload_started = true;
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

/// The player's weapons and which is out.
#[derive(Resource, Default)]
pub struct Arsenal {
    pub current: WeaponKind,
    /// Indexed by [`WeaponKind::slot`]. The machete's is unused.
    pub guns: [Gun; 5],
    /// Seconds since the current weapon came out.
    pub drawn_for: f32,
    /// Seconds since the last machete swing started.
    pub since_swing: f32,
    melee_cooldown: f32,
    /// Shots fired so far: turns the shotgun's pellet pattern each shot.
    shots: u32,
}

impl Arsenal {
    pub fn gun(&self) -> &Gun {
        &self.guns[self.current.slot()]
    }

    /// Mid-swing with the machete (including a quick slash with a gun out).
    pub fn swinging(&self) -> bool {
        self.since_swing < SWING_TIME
    }
}

// ---- firing ----

/// Effects state the fixed tick sets and the frame-rate systems animate.
#[derive(Resource, Default)]
pub struct GunFx {
    /// Recoil: how hard the last shot kicked (0..1), easing back to 0.
    pub kick: f32,
    /// Seconds of muzzle flash left.
    pub flash: f32,
    /// Seconds of hit marker left.
    pub hit_marker: f32,
}

// Rust note: Bevy systems ask for everything they use as parameters, so busy
// ones have a lot of them. That's normal in Bevy, so the lint is switched off.
#[allow(clippy::too_many_arguments)]
fn tick_weapons(
    mut commands: Commands,
    time: Res<Time>,
    handle: Res<WeaponsHandle>,
    tunings: Res<Assets<WeaponsAsset>>,
    sounds: Option<Res<Sounds>>,
    look: Res<EffectLook>,
    rocket_look: Res<RocketLook>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    camera: Single<(&Transform, &ActionState<Action>), With<PlayerCamera>>,
    map: Res<MapCollision>,
    targets: Query<(Entity, &Transform, &Hitbox)>,
    mut arsenal: ResMut<Arsenal>,
    mut fx: ResMut<GunFx>,
    mut hits: MessageWriter<Damage>,
) {
    let Some(tuning) = tunings.get(&handle.0) else {
        return;
    };
    let dt = time.delta_secs();
    let (eye, actions) = camera.into_inner();
    // `play!(field)` plays that sound, if the sounds are ready.
    macro_rules! play {
        ($sound:ident) => {
            if let Some(sounds) = &sounds {
                sfx::play(&mut commands, &sounds.$sound);
            }
        };
    }

    // Switching: by slot, or stepping with the wheel. A reload in progress is
    // dropped; the new weapon takes a moment to come up.
    let slots = [
        Action::Weapon1,
        Action::Weapon2,
        Action::Weapon3,
        Action::Weapon4,
        Action::Weapon5,
    ];
    let mut wanted = slots
        .iter()
        .position(|slot| actions.just_pressed(slot))
        .map(|slot| WeaponKind::ALL[slot]);
    if actions.just_pressed(&Action::NextWeapon) {
        wanted = Some(arsenal.current.step(1));
    }
    if actions.just_pressed(&Action::PreviousWeapon) {
        wanted = Some(arsenal.current.step(-1));
    }
    if let Some(kind) = wanted
        && kind != arsenal.current
    {
        let old = arsenal.current.slot();
        arsenal.guns[old].reloading = None;
        arsenal.current = kind;
        arsenal.drawn_for = 0.0;
        play!(open);
    }
    arsenal.drawn_for += dt;
    arsenal.since_swing += dt;
    arsenal.melee_cooldown = (arsenal.melee_cooldown - dt).max(0.0);
    let ready = arsenal.drawn_for >= SWITCH_TIME;

    // The click that captures the mouse shouldn't also fire.
    let aiming = input::cursor_captured(&cursor);
    let pressed = aiming && actions.just_pressed(&Action::Fire);
    let held = aiming && actions.pressed(&Action::Fire);

    // The machete: F any time, or fire with the machete out.
    let slash = aiming && actions.just_pressed(&Action::Melee)
        || (arsenal.current == WeaponKind::Machete && pressed && ready);
    if slash && arsenal.melee_cooldown <= 0.0 && !arsenal.swinging() {
        arsenal.melee_cooldown = tuning.machete.interval;
        arsenal.since_swing = 0.0;
        play!(swish);
        if slash_hits(
            eye,
            &tuning.machete,
            &targets,
            &map,
            &mut hits,
            &mut commands,
            &look,
        ) {
            fx.hit_marker = HIT_MARKER_TIME;
            play!(chop);
        }
    }

    let kind = arsenal.current;
    let Some(rules) = tuning.rules(kind) else {
        return;
    };
    let automatic = tuning.hitscan(kind).is_some_and(|g| g.automatic);
    let trigger = ready && !arsenal.swinging() && if automatic { held } else { pressed };
    let reload = actions.just_pressed(&Action::Reload);
    let events = arsenal.guns[kind.slot()].tick(trigger, reload, &rules, dt);
    if events.reload_started {
        play!(open);
    }
    if events.reload_half {
        play!(load);
    }
    if events.reload_done {
        play!(close);
    }
    if !events.fired {
        return;
    }
    arsenal.shots += 1;
    fx.flash = MUZZLE_FLASH_TIME;
    fx.kick = match kind {
        WeaponKind::Revolver => 0.9,
        WeaponKind::Shotgun => 1.0,
        WeaponKind::TommyGun => 0.3,
        _ => 0.8,
    };

    if kind == WeaponKind::Launcher {
        rockets::launch(
            &mut commands,
            &rocket_look,
            eye.transform_point(TRACER_START * Vec3::new(1.0, 1.0, 0.6)),
            *eye.forward(),
            &tuning.launcher,
        );
        play!(launch);
        return;
    }

    let Some(gun) = tuning.hitscan(kind) else {
        return;
    };
    match kind {
        WeaponKind::Revolver => play!(revolver),
        WeaponKind::TommyGun => play!(tommy),
        _ => play!(shotgun),
    }
    let spin = arsenal.shots as f32 * 2.1;
    let pellets = pellet_directions(
        *eye.forward(),
        *eye.right(),
        *eye.up(),
        gun.pellets,
        gun.spread_deg,
        spin,
    );
    let muzzle = eye.transform_point(TRACER_START);
    let mut hit_target = false;
    for direction in pellets {
        let (end, hit) = trace(eye.translation, direction, gun, &map, &targets, &mut hits);
        hit_target |= hit.is_some_and(|on_target| on_target);
        match hit {
            Some(true) => spawn_puff(&mut commands, &look, &look.blood, end - direction * 2.0),
            Some(false) => spawn_puff(&mut commands, &look, &look.dust, end),
            None => {}
        }
        // Single-bullet guns leave a tracer; a shotgun's dozen would be noise.
        if gun.pellets == 1 {
            spawn_tracer(&mut commands, &look, muzzle, end);
        }
    }
    if hit_target {
        fx.hit_marker = HIT_MARKER_TIME;
        play!(hit);
    }
}

/// Follow one bullet: damage the first thing with a [`Hitbox`] it reaches, if a
/// wall doesn't come first. Returns where it stopped, and what it hit:
/// `Some(true)` a target, `Some(false)` a wall, `None` nothing in range.
fn trace(
    origin: Vec3,
    direction: Vec3,
    gun: &GunTuning,
    map: &MapCollision,
    targets: &Query<(Entity, &Transform, &Hitbox)>,
    hits: &mut MessageWriter<Damage>,
) -> (Vec3, Option<bool>) {
    let wall = map
        .0
        .cast_ray(origin, direction * gun.range)
        .map(|hit| (hit.fraction * gun.range, hit.normal));
    let target = targets
        .iter()
        .filter(|(_, _, hitbox)| hitbox.enabled)
        .filter_map(|(entity, transform, hitbox)| {
            let distance = ray_box(origin, direction, hitbox.centre(transform), hitbox.half)?;
            (distance <= gun.range).then_some((distance, entity))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));

    match (wall, target) {
        (_, Some((distance, entity))) if wall.is_none_or(|(w, _)| distance < w) => {
            hits.write(Damage {
                target: entity,
                amount: gun.damage,
                direction,
            });
            (origin + direction * distance, Some(true))
        }
        (Some((distance, normal)), _) => {
            (origin + direction * distance + normal * 1.5, Some(false))
        }
        _ => (origin + direction * gun.range, None),
    }
}

/// A machete slash: everything with a hitbox within reach and inside the arc in
/// front takes the damage. Returns whether anything was hit.
fn slash_hits(
    eye: &Transform,
    machete: &MeleeTuning,
    targets: &Query<(Entity, &Transform, &Hitbox)>,
    map: &MapCollision,
    hits: &mut MessageWriter<Damage>,
    commands: &mut Commands,
    look: &EffectLook,
) -> bool {
    let forward = *eye.forward();
    let half_arc = (machete.arc_deg * 0.5).to_radians();
    let mut hit_any = false;
    for (entity, transform, hitbox) in targets {
        if !hitbox.enabled {
            continue;
        }
        let centre = hitbox.centre(transform);
        // Distance to the nearest point of the box, so big things are easier to hit.
        let nearest = eye
            .translation
            .clamp(centre - hitbox.half, centre + hitbox.half);
        if eye.translation.distance(nearest) > machete.reach {
            continue;
        }
        let towards = (nearest - eye.translation).normalize_or(forward);
        if forward.angle_between(towards) > half_arc {
            continue;
        }
        hits.write(Damage {
            target: entity,
            amount: machete.damage,
            direction: forward,
        });
        spawn_puff(commands, look, &look.blood, nearest);
        hit_any = true;
    }
    // Nothing alive in reach: a wall in front still throws up dust.
    if !hit_any && let Some(hit) = map.0.cast_ray(eye.translation, forward * machete.reach) {
        let point = eye.translation + forward * machete.reach * hit.fraction + hit.normal * 1.5;
        spawn_puff(commands, look, &look.dust, point);
    }
    hit_any
}

// ---- impact puffs and tracers ----

#[derive(Resource)]
pub struct EffectLook {
    puff: Handle<Mesh>,
    dust: Handle<StandardMaterial>,
    blood: Handle<StandardMaterial>,
    tracer: Handle<StandardMaterial>,
}

/// A little cloud where a shot hit. Pops up, then shrinks away.
#[derive(Component)]
struct Puff {
    life: f32,
}

/// A bright streak along a bullet's path, gone almost at once.
#[derive(Component)]
struct Tracer {
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
        tracer: materials.add(flat(Color::srgb(1.0, 0.9, 0.55))),
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

fn spawn_tracer(commands: &mut Commands, look: &EffectLook, from: Vec3, to: Vec3) {
    if from.distance(to) < 1.0 {
        return;
    }
    commands.spawn((
        Tracer { life: TRACER_LIFE },
        Mesh3d(look.puff.clone()),
        MeshMaterial3d(look.tracer.clone()),
        // The puff mesh is a 4-unit cube: scale it to a thin line.
        character::limb(from, to, Vec2::splat(0.6)).with_scale(Vec3::new(
            0.15,
            0.15,
            from.distance(to) / 4.0,
        )),
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

fn fade_tracers(
    mut commands: Commands,
    time: Res<Time>,
    mut tracers: Query<(Entity, &mut Tracer)>,
) {
    for (entity, mut tracer) in &mut tracers {
        tracer.life -= time.delta_secs();
        if tracer.life <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
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
            font_size: FontSize::Px(26.0),
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.85, 0.65)),
        TextLayout::justify(Justify::Right),
        Node {
            position_type: PositionType::Absolute,
            right: px(28),
            bottom: px(22),
            ..default()
        },
    ));
}

fn update_hud(
    arsenal: Res<Arsenal>,
    fx: Res<GunFx>,
    handle: Res<WeaponsHandle>,
    tunings: Res<Assets<WeaponsAsset>>,
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

    // Slot strip, current one bracketed: "1 2 [3] 4 5".
    let slots: Vec<String> = WeaponKind::ALL
        .iter()
        .map(|kind| {
            let n = kind.slot() + 1;
            if *kind == arsenal.current {
                format!("[{n}]")
            } else {
                format!("{n}")
            }
        })
        .collect();
    let kind = arsenal.current;
    let rounds = match tunings.get(&handle.0).and_then(|t| t.rules(kind)) {
        None => String::new(),
        Some(_) if arsenal.gun().reloading.is_some() => "  RELOADING".to_string(),
        Some(rules) => format!(
            "  {}/{}",
            arsenal.gun().loaded.min(rules.magazine),
            rules.magazine
        ),
    };
    ammo.0 = format!("{}\n{}{rounds}", slots.join(" "), kind.name());
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn rules() -> GunRules {
        GunRules {
            magazine: 2,
            interval: 0.2,
            reload_time: 1.0,
        }
    }

    /// Run `seconds` of ticks with no input, collecting what happened.
    fn wait(gun: &mut Gun, seconds: f32) -> Vec<GunEvents> {
        (0..(seconds / DT).round() as usize)
            .map(|_| gun.tick(false, false, &rules(), DT))
            .collect()
    }

    #[test]
    fn shipped_tuning_parses() {
        let t: WeaponsTuning = ron::from_str(include_str!("../assets/weapons.ron")).unwrap();
        assert_eq!(t.shotgun.magazine, 2);
        for kind in WeaponKind::ALL {
            if let Some(rules) = t.rules(kind) {
                assert!(
                    rules.magazine > 0 && rules.reload_time > rules.interval,
                    "{kind:?}"
                );
            }
        }
        assert!(t.tommy_gun.automatic && !t.revolver.automatic);
    }

    #[test]
    fn one_shot_per_trigger_then_it_reloads_itself() {
        let r = rules();
        let mut gun = Gun::default();
        assert!(gun.tick(true, false, &r, DT).fired);
        assert_eq!(gun.loaded, 1);
        // Pulling again straight away does nothing: not ready yet.
        assert!(!gun.tick(true, false, &r, DT).fired);
        wait(&mut gun, 0.2);
        assert!(gun.tick(true, false, &r, DT).fired);
        assert_eq!(gun.loaded, 0);

        // Empty: it starts reloading by itself once the cooldown is over...
        let events = wait(&mut gun, 0.25);
        assert_eq!(events.iter().filter(|e| e.reload_started).count(), 1);
        // ...can't fire while reloading...
        assert!(!gun.tick(true, false, &r, DT).fired);
        // ...and is full again after the reload time.
        let events = wait(&mut gun, 1.0);
        assert_eq!(events.iter().filter(|e| e.reload_half).count(), 1);
        assert_eq!(events.iter().filter(|e| e.reload_done).count(), 1);
        assert_eq!((gun.loaded, gun.reloading), (2, None));
    }

    #[test]
    fn holding_the_trigger_fires_at_the_fire_rate() {
        let r = GunRules {
            magazine: 50,
            interval: 0.1,
            reload_time: 2.0,
        };
        let mut gun = Gun::default();
        let fired = (0..60)
            .filter(|_| gun.tick(true, false, &r, DT).fired)
            .count();
        // One second at 10 rounds a second (give or take a tick of rounding).
        assert!((10..=11).contains(&fired), "{fired}");
    }

    #[test]
    fn reload_button_tops_up_but_not_when_full() {
        let r = rules();
        let mut gun = Gun::default();
        assert!(
            !gun.tick(false, true, &r, DT).reload_started,
            "reloaded when full"
        );
        gun.tick(true, false, &r, DT);
        wait(&mut gun, 0.3);
        assert_eq!(gun.reloading, None, "one round left shouldn't auto-reload");
        assert!(gun.tick(false, true, &r, DT).reload_started);
        // A tick over the reload time: 60 ticks of 1/60 s add up to a hair under 1.
        wait(&mut gun, 1.0 + DT);
        assert_eq!(gun.loaded, 2);
    }

    #[test]
    fn weapon_slots_wrap_round() {
        assert_eq!(WeaponKind::Launcher.step(1), WeaponKind::Machete);
        assert_eq!(WeaponKind::Machete.step(-1), WeaponKind::Launcher);
        assert_eq!(WeaponKind::Revolver.step(1), WeaponKind::Shotgun);
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

    #[test]
    fn a_single_bullet_with_no_spread_goes_straight() {
        let dirs: Vec<Vec3> =
            pellet_directions(Vec3::NEG_Z, Vec3::X, Vec3::Y, 1, 0.0, 1.3).collect();
        assert_eq!(dirs, vec![Vec3::NEG_Z]);
    }
}
