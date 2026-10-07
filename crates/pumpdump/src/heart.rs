//! Your heart. You're a zombie, so it doesn't beat by itself: it holds a
//! measure of blood that drains away, and you keep it going by pulling it out
//! of your chest (Q) and squeezing it (fire). If it runs dry you flatline and
//! lose health until you beat it again.
//!
//! At the start of each run you pick how fast it drains (its heart rate, in
//! `assets/heart.ron`). Faster means more squeezing, but bigger boosts: damage,
//! accuracy, health, speed, reload speed. The fastest rate also gets a perk.
//! As the heart runs low the boosts fade (all but health); squeezing brings
//! them back. See `run.rs` for picking.

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
use crate::upgrades::Upgrades;
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
    /// At or above this much blood the boosts are at full power; below it they
    /// fade, to nothing on an empty heart.
    pub full_power_above: f32,
    /// The heart rates to choose from, slowest first.
    pub tiers: Vec<Tier>,
    pub bloodlust_heal: f32,
    pub pulse_damage: f32,
    pub pulse_radius: f32,
    /// Seconds before another squeeze can pulse.
    pub pulse_cooldown: f32,
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
    /// Aim assist, degrees: shots this close to a target bend onto it.
    pub assist: f32,
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
                "Squeezing your heart (when it needs it) blasts everything within {:.0} for {:.0}. Once every {:.0} s.",
                tuning.pulse_radius, tuning.pulse_damage, tuning.pulse_cooldown
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
    /// Times the Second Heart has brought you back this level.
    pub revives_used: u32,
    /// The heart rate's boosts at full power. `Boosts` (the resource) is
    /// these faded by how much blood is left.
    pub boosts: Boosts,
}

/// What the run's heart rate does for you right now. Weapons, movement and
/// health read this. The heart keeps it up to date: see `Boosts::faded`.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Boosts {
    pub damage: f32,
    /// Weapon spread multiplier (lower is more accurate).
    pub spread: f32,
    /// Reload time multiplier (lower is faster).
    pub reload: f32,
    pub speed: f32,
    pub max_health: f32,
    /// Aim assist, degrees (see `weapon::bend`).
    pub assist: f32,
}

impl Default for Boosts {
    fn default() -> Self {
        Self {
            damage: 1.0,
            spread: 1.0,
            reload: 1.0,
            speed: 1.0,
            max_health: 100.0,
            assist: 0.0,
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
            assist: tier.assist,
        }
    }

    /// These boosts at `power` (0 none, 1 full): every multiplier slides
    /// towards 1. Maximum health stays put, so a faltering heart never takes
    /// health away on top of the flatline bleed.
    pub fn faded(&self, power: f32) -> Self {
        let p = power.clamp(0.0, 1.0);
        let fade = |x: f32| 1.0 + (x - 1.0) * p;
        Self {
            damage: fade(self.damage),
            spread: fade(self.spread),
            reload: fade(self.reload),
            speed: fade(self.speed),
            max_health: self.max_health,
            assist: self.assist * p,
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
    /// Seconds since the last Pulse shockwave.
    pub since_pulse: f32,
    /// Seconds since the last squeeze that put blood back.
    pub since_pump: f32,
    /// Seconds since the last heartbeat (the sound): the animations keep time
    /// with it.
    pub since_beat: f32,
    /// The run's heart rate, for the sound and the animation.
    pub bpm: f32,
}

impl Default for Heart {
    fn default() -> Self {
        Self {
            blood: 1.0,
            held: false,
            since_squeeze: 10.0,
            since_pulse: 10.0,
            since_pump: 10.0,
            since_beat: 10.0,
            bpm: 30.0,
        }
    }
}

impl Heart {
    /// Drain for `dt` seconds, for a heart whose full measure `lasts` seconds.
    pub fn drain(&mut self, dt: f32, lasts: f32) {
        self.blood = (self.blood - dt / lasts.max(0.01)).max(0.0);
    }

    /// One squeeze: a beat's worth of blood back, never past full. Returns
    /// whether it did any good (the heart wasn't already full).
    pub fn squeeze(&mut self, amount: f32) -> bool {
        let was = self.blood;
        self.blood = (self.blood + amount).min(1.0);
        self.since_squeeze = 0.0;
        self.blood > was
    }

    pub fn flatlined(&self) -> bool {
        self.blood <= 0.0
    }

    /// How much of the heart rate's boosts you're getting: full at or above
    /// `full_above` blood, fading to none on an empty heart.
    pub fn power(&self, full_above: f32) -> f32 {
        (self.blood / full_above.max(1e-3)).clamp(0.0, 1.0)
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
    (run, upgrades): (Res<Run>, Res<Upgrades>),
    sounds: Option<Res<Sounds>>,
    look: Res<PulseLook>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    actions: Single<&ActionState<Action>, With<PlayerCamera>>,
    player: Res<PlayerStatus>,
    targets: Query<(Entity, &Transform, &Hitbox)>,
    mut heart: ResMut<Heart>,
    mut boosts: ResMut<Boosts>,
    mut health: ResMut<PlayerHealth>,
    mut arsenal: ResMut<Arsenal>,
    mut hits: MessageWriter<Damage>,
) {
    let (Some(tuning), true) = (tunings.get(&handle.0), run.started) else {
        return;
    };
    // The perk's upgrades, if any.
    let tuning = &upgrades.perk_tuning(tuning);
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
    heart.since_pulse += dt;
    heart.since_pump += dt;
    let squeeze = heart.held
        && arsenal.drawn_for > 0.15
        && input::cursor_captured(&cursor)
        && actions.just_pressed(&Action::Fire);
    if squeeze {
        let pumped = heart.squeeze(tuning.squeeze);
        if pumped {
            heart.since_pump = 0.0;
        }
        if let Some(sounds) = &sounds {
            sfx::play(&mut commands, &sounds.squelch);
        }
        // Pulse: only for a beat the heart needed (no spamming a full heart),
        // and not more often than its cooldown.
        if run.perk == Some(Perk::Pulse) && pumped && heart.since_pulse >= tuning.pulse_cooldown {
            heart.since_pulse = 0.0;
            pulse(&mut commands, &look, tuning, &player, &targets, &mut hits);
            if let Some(sounds) = &sounds {
                sfx::play(&mut commands, &sounds.thud);
            }
        }
    }
    // A heart running low saps your boosts; squeezing brings them back.
    *boosts = run.boosts.faded(heart.power(tuning.full_power_above));
    // Flatlining: a steady bleed, with no healing while it lasts.
    if heart.flatlined() {
        health.bleed(tuning.flatline_damage * dt);
    }
}

/// The Pulse perk: hit everything near the player, and draw the ring.
fn pulse(
    commands: &mut Commands,
    look: &PulseLook,
    tuning: &HeartTuning,
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
                // Not boosted by the heart rate: it's a perk on top.
                amount: tuning.pulse_damage,
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

/// The Bloodlust perk: every kill heals (and, upgraded, feeds your heart).
fn bloodlust(
    run: Res<Run>,
    upgrades: Res<Upgrades>,
    handle: Res<HeartHandle>,
    tunings: Res<Assets<HeartAsset>>,
    mut kills: MessageReader<EnemyKilled>,
    mut health: ResMut<PlayerHealth>,
    mut heart: ResMut<Heart>,
) {
    let killed = kills.read().count();
    if killed == 0 || run.perk != Some(Perk::Bloodlust) {
        return;
    }
    if let Some(tuning) = tunings.get(&handle.0) {
        let tuning = upgrades.perk_tuning(tuning);
        let max = health.max;
        health.current = (health.current + tuning.bloodlust_heal * killed as f32).min(max);
        if upgrades.perk >= 2 {
            heart.blood = (heart.blood + crate::upgrades::FRENZY_BLOOD * killed as f32).min(1.0);
        }
    }
}

/// Lub-dub at the heart rate, as long as there's blood in it. Also times the
/// HUD's pulse.
fn heartbeat(
    mut commands: Commands,
    time: Res<Time>,
    run: Res<Run>,
    mut heart: ResMut<Heart>,
    sounds: Option<Res<Sounds>>,
    mut until_beat: Local<f32>,
) {
    heart.since_beat += time.delta_secs();
    if !run.started || heart.flatlined() {
        return;
    }
    *until_beat -= time.delta_secs();
    if *until_beat <= 0.0 {
        *until_beat += 60.0 / heart.bpm.max(1.0);
        // Don't fall behind after a pause.
        *until_beat = until_beat.max(0.0);
        heart.since_beat = 0.0;
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
    let power = heart.power(tuning.full_power_above);
    let fading = if power < 1.0 {
        format!("  POWER {:.0}%", power * 100.0)
    } else {
        String::new()
    };
    label.0 = format!("HEART  {tier}  {:.0} BPM{perk}{fading}{hand}", heart.bpm);
    fill_node.width = percent(heart.blood * 100.0);

    // The bar throbs with the heart rate.
    let t = real.elapsed_secs();
    // In time with the heartbeat.
    let throb = (1.0 - heart.since_beat * heart.bpm / 60.0 * 4.0).max(0.0);
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
            assert!(fast.assist >= slow.assist);
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
        assert!(heart.squeeze(0.25));
        assert!((heart.blood - 0.25).abs() < 1e-6);
        for _ in 0..10 {
            heart.squeeze(0.25);
        }
        assert_eq!(heart.blood, 1.0);
        // A full heart gains nothing from another squeeze.
        assert!(!heart.squeeze(0.25));
        assert_eq!(heart.since_squeeze, 0.0);
    }

    #[test]
    fn boosts_fade_as_the_heart_empties_and_come_back_with_a_squeeze() {
        let t = tuning();
        let full = Boosts::from_tier(t.tiers.last().unwrap());
        let mut heart = Heart::default();
        // A full heart, and anything above the threshold: full power.
        assert_eq!(heart.power(t.full_power_above), 1.0);
        assert_eq!(full.faded(heart.power(t.full_power_above)), full);
        // Halfway below it: halfway back to normal.
        heart.blood = t.full_power_above / 2.0;
        let half = full.faded(heart.power(t.full_power_above));
        assert!((half.damage - (1.0 + (full.damage - 1.0) / 2.0)).abs() < 1e-5);
        assert!(half.reload > full.reload && half.reload < 1.0);
        assert!(half.spread > full.spread && half.speed < full.speed);
        // Empty: no boosts at all, but maximum health stays.
        heart.blood = 0.0;
        let none = full.faded(heart.power(t.full_power_above));
        assert_eq!(
            none,
            Boosts {
                max_health: full.max_health,
                ..default()
            }
        );
        // Squeezing puts power straight back.
        heart.squeeze(t.squeeze);
        assert!(heart.power(t.full_power_above) > 0.0);
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
