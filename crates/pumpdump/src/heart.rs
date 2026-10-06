//! Your heart. You're a zombie, so it doesn't beat by itself: it holds a
//! measure of blood that drains away, and you keep it going by pulling it out
//! of your chest (Q) and squeezing it (fire). If it runs dry you flatline and
//! lose health until you beat it again.
//!
//! At the start of each run you pick how fast it drains (its heart rate, in
//! `assets/heart.ron`). Faster means more squeezing, but bigger boosts: damage,
//! accuracy, health, speed, reload speed. The fastest rate also gets a perk.
//! See `run.rs` for picking.

use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use leafwing_input_manager::prelude::*;
use pumpdump_movement::MovementTuning;
use serde::Deserialize;

use crate::enemies::EnemyKilled;
use crate::health::PlayerHealth;
use crate::input::{self, Action};
use crate::player::{MovementTick, PlayerCamera, PlayerStatus};
use crate::sfx::{self, Sounds};
use crate::targets::{Damage, Hitbox};
use crate::weapon::Arsenal;

pub struct HeartPlugin;

impl Plugin for HeartPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<HeartAsset>()
            .register_asset_loader(HeartLoader)
            .init_resource::<Run>()
            .init_resource::<Boosts>()
            .init_resource::<Heart>()
            .add_systems(Startup, (load_tuning, make_pulse_look, spawn_hud))
            .add_systems(
                FixedUpdate,
                (tick_heart, bloodlust).chain().after(MovementTick),
            )
            .add_systems(Update, (heartbeat, update_hud, grow_pulses));
    }
}

/// Seconds a Pulse shockwave takes to spread out.
const PULSE_TIME: f32 = 0.3;

// ---- tuning ----

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeartTuning {
    /// One squeeze refills this much of the heart (1 = full).
    pub squeeze: f32,
    /// Health lost per second while flatlining.
    pub flatline_damage: f32,
    /// Below this much blood, the game warns you.
    pub warn_below: f32,
    /// The heart rates to choose from, slowest first.
    pub tiers: Vec<Tier>,
    pub bloodlust_heal: f32,
    pub pulse_damage: f32,
    pub pulse_radius: f32,
    pub second_heart_health: f32,
}

/// One heart rate and what it does for you.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tier {
    pub name: String,
    pub bpm: u32,
    /// Seconds a full heart lasts.
    pub lasts: f32,
    pub damage: f32,
    pub spread: f32,
    pub health: f32,
    pub speed: f32,
    pub reload: f32,
}

#[derive(Asset, TypePath, Deref)]
pub struct HeartAsset(pub HeartTuning);

#[derive(Resource)]
pub struct HeartHandle(pub Handle<HeartAsset>);

#[derive(TypePath)]
struct HeartLoader;

impl AssetLoader for HeartLoader {
    type Asset = HeartAsset;
    type Settings = ();
    type Error = Box<dyn std::error::Error + Send + Sync>;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<HeartAsset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(HeartAsset(ron::de::from_bytes(&bytes)?))
    }

    fn extensions(&self) -> &[&str] {
        &["heart.ron"]
    }
}

fn load_tuning(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(HeartHandle(assets.load("heart.ron")));
}

impl HeartTuning {
    /// The tier at `index`, or the fastest if the file now has fewer.
    pub fn tier(&self, index: usize) -> Option<&Tier> {
        self.tiers
            .get(index.min(self.tiers.len().saturating_sub(1)))
    }
}

// ---- the run ----

/// Perks for the fastest heart rate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Perk {
    /// Kills heal you.
    Bloodlust,
    /// Each squeeze sends out a shockwave that hurts everything near you.
    Pulse,
    /// The first death of a run doesn't count.
    SecondHeart,
}

impl Perk {
    pub const ALL: [Perk; 3] = [Perk::Bloodlust, Perk::Pulse, Perk::SecondHeart];

    pub fn name(self) -> &'static str {
        match self {
            Perk::Bloodlust => "BLOODLUST",
            Perk::Pulse => "PULSE",
            Perk::SecondHeart => "SECOND HEART",
        }
    }

    pub fn description(self, tuning: &HeartTuning) -> String {
        match self {
            Perk::Bloodlust => format!("Every kill heals you {:.0}.", tuning.bloodlust_heal),
            Perk::Pulse => format!(
                "Every squeeze of your heart blasts everything within {:.0} for {:.0}.",
                tuning.pulse_radius, tuning.pulse_damage
            ),
            Perk::SecondHeart => format!(
                "The first time you die, you get back up with {:.0}% health.",
                tuning.second_heart_health * 100.0
            ),
        }
    }
}

/// The current run: the heart rate picked for it, and the perk if any.
#[derive(Resource, Default)]
pub struct Run {
    /// Playing (rather than on the start screen).
    pub started: bool,
    /// Index into `HeartTuning::tiers`.
    pub tier: usize,
    pub perk: Option<Perk>,
    pub second_heart_used: bool,
}

/// What the run's heart rate does for you. Weapons, movement and health read
/// this.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Boosts {
    pub damage: f32,
    /// Weapon spread multiplier (lower is more accurate).
    pub spread: f32,
    /// Reload time multiplier (lower is faster).
    pub reload: f32,
    pub speed: f32,
    pub max_health: f32,
}

impl Default for Boosts {
    fn default() -> Self {
        Self {
            damage: 1.0,
            spread: 1.0,
            reload: 1.0,
            speed: 1.0,
            max_health: 100.0,
        }
    }
}

impl Boosts {
    pub fn from_tier(tier: &Tier) -> Self {
        Self {
            damage: tier.damage,
            spread: tier.spread,
            reload: tier.reload,
            speed: tier.speed,
            max_health: tier.health,
        }
    }

    /// The movement tuning with the speed boost applied: running, sliding,
    /// wall-running and dashing all go faster (the speed cap rises to match).
    pub fn movement(&self, tuning: &MovementTuning) -> MovementTuning {
        let s = self.speed;
        MovementTuning {
            walk_speed: tuning.walk_speed * s,
            sprint_speed: tuning.sprint_speed * s,
            slide_speed: tuning.slide_speed * s,
            wall_run_speed: tuning.wall_run_speed * s,
            dash_speed: tuning.dash_speed * s,
            dash_exit_speed: tuning.dash_exit_speed * s,
            max_speed: tuning.max_speed * s,
            ..tuning.clone()
        }
    }
}

// ---- the heart ----

#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Heart {
    /// How full it is: 1 full, 0 empty (flatlining).
    pub blood: f32,
    /// In your hand (rather than your chest): your weapons are put away.
    pub held: bool,
    /// Seconds since the last squeeze.
    pub since_squeeze: f32,
    /// The run's heart rate, for the sound and the animation.
    pub bpm: f32,
}

impl Default for Heart {
    fn default() -> Self {
        Self {
            blood: 1.0,
            held: false,
            since_squeeze: 10.0,
            bpm: 30.0,
        }
    }
}

impl Heart {
    /// Drain for `dt` seconds, for a heart whose full measure `lasts` seconds.
    pub fn drain(&mut self, dt: f32, lasts: f32) {
        self.blood = (self.blood - dt / lasts.max(0.01)).max(0.0);
    }

    /// One squeeze: a beat's worth of blood back, never past full.
    pub fn squeeze(&mut self, amount: f32) {
        self.blood = (self.blood + amount).min(1.0);
        self.since_squeeze = 0.0;
    }

    pub fn flatlined(&self) -> bool {
        self.blood <= 0.0
    }
}

/// A Pulse shockwave: a ring spreading out along the ground.
#[derive(Component)]
struct PulseRing {
    age: f32,
    radius: f32,
}

#[derive(Resource)]
struct PulseLook {
    disc: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

fn make_pulse_look(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(PulseLook {
        disc: meshes.add(Cylinder::new(1.0, 1.0)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.75, 0.05, 0.08),
            unlit: true,
            ..default()
        }),
    });
}

#[allow(clippy::too_many_arguments)]
fn tick_heart(
    mut commands: Commands,
    time: Res<Time>,
    handle: Res<HeartHandle>,
    tunings: Res<Assets<HeartAsset>>,
    run: Res<Run>,
    boosts: Res<Boosts>,
    sounds: Option<Res<Sounds>>,
    look: Res<PulseLook>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    actions: Single<&ActionState<Action>, With<PlayerCamera>>,
    player: Res<PlayerStatus>,
    targets: Query<(Entity, &Transform, &Hitbox)>,
    mut heart: ResMut<Heart>,
    mut health: ResMut<PlayerHealth>,
    mut arsenal: ResMut<Arsenal>,
    mut hits: MessageWriter<Damage>,
) {
    let (Some(tuning), true) = (tunings.get(&handle.0), run.started) else {
        return;
    };
    let Some(tier) = tuning.tier(run.tier) else {
        return;
    };
    let dt = time.delta_secs();

    // Q pulls the heart out or puts it back. Picking a weapon puts it back too.
    let switching = [
        Action::Weapon1,
        Action::Weapon2,
        Action::Weapon3,
        Action::Weapon4,
        Action::Weapon5,
        Action::NextWeapon,
        Action::PreviousWeapon,
    ]
    .iter()
    .any(|a| actions.just_pressed(a));
    let toggle = actions.just_pressed(&Action::Heart);
    if toggle || (switching && heart.held) {
        heart.held = toggle && !heart.held;
        // The hands swap over: the same raise as switching weapons.
        arsenal.drawn_for = 0.0;
        if let Some(sounds) = &sounds {
            sfx::play(&mut commands, &sounds.load);
        }
    }

    heart.drain(dt, tier.lasts);
    heart.since_squeeze += dt;
    let squeeze = heart.held
        && arsenal.drawn_for > 0.15
        && input::cursor_captured(&cursor)
        && actions.just_pressed(&Action::Fire);
    if squeeze {
        heart.squeeze(tuning.squeeze);
        if let Some(sounds) = &sounds {
            sfx::play(&mut commands, &sounds.squelch);
        }
        if run.perk == Some(Perk::Pulse) {
            pulse(
                &mut commands,
                &look,
                tuning,
                &boosts,
                &player,
                &targets,
                &mut hits,
            );
            if let Some(sounds) = &sounds {
                sfx::play(&mut commands, &sounds.thud);
            }
        }
    }
    if heart.flatlined() {
        health.current -= tuning.flatline_damage * dt;
    }
}

/// The Pulse perk: hit everything near the player, and draw the ring.
fn pulse(
    commands: &mut Commands,
    look: &PulseLook,
    tuning: &HeartTuning,
    boosts: &Boosts,
    player: &PlayerStatus,
    targets: &Query<(Entity, &Transform, &Hitbox)>,
    hits: &mut MessageWriter<Damage>,
) {
    let middle = player.position + Vec3::Y * 36.0;
    for (target, transform, hitbox) in targets {
        if !hitbox.enabled {
            continue;
        }
        let centre = hitbox.centre(transform);
        let nearest = middle.clamp(centre - hitbox.half, centre + hitbox.half);
        if middle.distance(nearest) <= tuning.pulse_radius {
            hits.write(Damage {
                target,
                amount: tuning.pulse_damage * boosts.damage,
                direction: (centre - middle).normalize_or(Vec3::Y),
            });
        }
    }
    commands.spawn((
        PulseRing {
            age: 0.0,
            radius: tuning.pulse_radius,
        },
        Mesh3d(look.disc.clone()),
        MeshMaterial3d(look.material.clone()),
        Transform::from_translation(player.position + Vec3::Y * 2.0)
            .with_scale(Vec3::new(1.0, 1.0, 1.0)),
        NotShadowCaster,
    ));
}

fn grow_pulses(
    mut commands: Commands,
    time: Res<Time>,
    mut rings: Query<(Entity, &mut PulseRing, &mut Transform)>,
) {
    for (entity, mut ring, mut transform) in &mut rings {
        ring.age += time.delta_secs();
        let t = ring.age / PULSE_TIME;
        if t >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        // A wide, thin disc spreading out to the pulse's radius and thinning away.
        transform.scale = Vec3::new(ring.radius * t, 3.0 * (1.0 - t), ring.radius * t);
    }
}

/// The Bloodlust perk: every kill heals.
fn bloodlust(
    run: Res<Run>,
    handle: Res<HeartHandle>,
    tunings: Res<Assets<HeartAsset>>,
    mut kills: MessageReader<EnemyKilled>,
    mut health: ResMut<PlayerHealth>,
) {
    let killed = kills.read().count();
    if killed == 0 || run.perk != Some(Perk::Bloodlust) {
        return;
    }
    if let Some(tuning) = tunings.get(&handle.0) {
        let max = health.max;
        health.current = (health.current + tuning.bloodlust_heal * killed as f32).min(max);
    }
}

/// Lub-dub at the heart rate, as long as there's blood in it. Also times the
/// HUD's pulse.
fn heartbeat(
    mut commands: Commands,
    time: Res<Time>,
    run: Res<Run>,
    heart: Res<Heart>,
    sounds: Option<Res<Sounds>>,
    mut until_beat: Local<f32>,
) {
    if !run.started || heart.flatlined() {
        return;
    }
    *until_beat -= time.delta_secs();
    if *until_beat <= 0.0 {
        *until_beat += 60.0 / heart.bpm.max(1.0);
        // Don't fall behind after a pause.
        *until_beat = until_beat.max(0.0);
        if let Some(sounds) = &sounds {
            sfx::play(&mut commands, &sounds.heartbeat);
        }
    }
}

// ---- HUD ----

#[derive(Component)]
struct HeartLabel;

#[derive(Component)]
struct HeartFill;

#[derive(Component)]
struct HeartWarning;

/// Darkens the screen while flatlining.
#[derive(Component)]
struct FlatlineShade;

const BAR_WIDTH: f32 = 260.0;

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        FlatlineShade,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundColor(Color::NONE),
    ));
    // Bottom middle: name and rate, the blood bar, and a warning line.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            bottom: px(18),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(4),
            ..default()
        })
        .with_children(|hud| {
            hud.spawn((
                HeartWarning,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(22.0),
                    ..default()
                },
                TextColor(Color::srgb(1.0, 0.25, 0.2)),
            ));
            hud.spawn((
                HeartLabel,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                TextColor(Color::srgb(0.95, 0.6, 0.6)),
            ));
            hud.spawn((
                Node {
                    width: px(BAR_WIDTH),
                    height: px(12),
                    border: UiRect::all(px(2)),
                    ..default()
                },
                BorderColor::all(Color::srgb(0.4, 0.08, 0.08)),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
            ))
            .with_child((
                HeartFill,
                Node {
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.75, 0.06, 0.08)),
            ));
        });
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_hud(
    real: Res<Time<Real>>,
    run: Res<Run>,
    heart: Res<Heart>,
    handle: Res<HeartHandle>,
    tunings: Res<Assets<HeartAsset>>,
    mut label: Single<&mut Text, (With<HeartLabel>, Without<HeartWarning>)>,
    mut warning: Single<&mut Text, (With<HeartWarning>, Without<HeartLabel>)>,
    fill: Single<(&mut Node, &mut BackgroundColor), (With<HeartFill>, Without<FlatlineShade>)>,
    mut shade: Single<&mut BackgroundColor, (With<FlatlineShade>, Without<HeartFill>)>,
) {
    let Some(tuning) = tunings.get(&handle.0) else {
        return;
    };
    let (mut fill_node, mut fill_colour) = fill.into_inner();
    if !run.started {
        label.0.clear();
        warning.0.clear();
        shade.0 = Color::NONE;
        return;
    }
    let tier = tuning.tier(run.tier).map_or("", |t| t.name.as_str());
    let perk = run
        .perk
        .map(|p| format!("  +{}", p.name()))
        .unwrap_or_default();
    let hand = if heart.held {
        "  [Q] put away  [fire] squeeze"
    } else {
        "  [Q] pull out"
    };
    label.0 = format!("HEART  {tier}  {:.0} BPM{perk}{hand}", heart.bpm);
    fill_node.width = percent(heart.blood * 100.0);

    // The bar throbs with the heart rate.
    let t = real.elapsed_secs();
    let beat = (t * heart.bpm / 60.0).fract();
    let throb = (1.0 - beat * 4.0).max(0.0);
    fill_colour.0 = Color::srgb(0.6 + 0.35 * throb, 0.05, 0.07);

    let blink = (t * 4.0).fract() < 0.5;
    warning.0 = if heart.flatlined() {
        "FLATLINE  -  BEAT YOUR HEART  [Q]".to_string()
    } else if heart.blood < tuning.warn_below && blink {
        "BEAT YOUR HEART  [Q]".to_string()
    } else {
        String::new()
    };
    shade.0 = if heart.flatlined() {
        Color::srgba(0.0, 0.0, 0.0, 0.45 + 0.1 * (t * 3.0).sin())
    } else {
        Color::NONE
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> HeartTuning {
        ron::from_str(include_str!("../assets/heart.ron")).unwrap()
    }

    #[test]
    fn shipped_tuning_parses_and_faster_is_stronger() {
        let t = tuning();
        assert_eq!(t.tiers.len(), 5);
        for pair in t.tiers.windows(2) {
            let (slow, fast) = (&pair[0], &pair[1]);
            assert!(fast.bpm > slow.bpm && fast.lasts < slow.lasts, "{fast:?}");
            assert!(fast.damage >= slow.damage && fast.health >= slow.health);
            assert!(fast.speed >= slow.speed);
            assert!(fast.spread <= slow.spread && fast.reload <= slow.reload);
        }
    }

    #[test]
    fn a_full_heart_lasts_its_time_then_flatlines() {
        let mut heart = Heart::default();
        for _ in 0..(10.0 * 60.0) as usize - 1 {
            heart.drain(1.0 / 60.0, 10.0);
        }
        assert!(!heart.flatlined() && heart.blood < 0.01, "{heart:?}");
        heart.drain(1.0 / 60.0, 10.0);
        heart.drain(1.0 / 60.0, 10.0);
        assert!(heart.flatlined());
    }

    #[test]
    fn squeezes_refill_but_never_past_full() {
        let mut heart = Heart {
            blood: 0.0,
            ..default()
        };
        heart.squeeze(0.25);
        assert!((heart.blood - 0.25).abs() < 1e-6);
        for _ in 0..10 {
            heart.squeeze(0.25);
        }
        assert_eq!(heart.blood, 1.0);
        assert_eq!(heart.since_squeeze, 0.0);
    }

    #[test]
    fn speed_boost_speeds_up_every_way_of_moving() {
        let movement: MovementTuning =
            ron::from_str(include_str!("../assets/movement.ron")).unwrap();
        let boosts = Boosts {
            speed: 1.2,
            ..default()
        };
        let fast = boosts.movement(&movement);
        assert!((fast.walk_speed - movement.walk_speed * 1.2).abs() < 1e-3);
        assert!((fast.dash_speed - movement.dash_speed * 1.2).abs() < 1e-3);
        assert!((fast.wall_run_speed - movement.wall_run_speed * 1.2).abs() < 1e-3);
        assert_eq!(fast.gravity, movement.gravity, "only speeds change");
    }
}
