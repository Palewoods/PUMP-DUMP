//! The player's health: taking hits, a red flash, slowly healing when left
//! alone, and dying. The run's heart rate sets the maximum (see `heart.rs`);
//! dying ends the run (see `run.rs`).

use bevy::prelude::*;

use crate::player::MovementTick;
use crate::sfx::{self, Sounds};

pub struct HealthPlugin;

impl Plugin for HealthPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerHealth>()
            .add_message::<PlayerHit>()
            .add_message::<PlayerDied>()
            .add_systems(Startup, spawn_hud)
            .add_systems(FixedUpdate, take_hits.after(MovementTick))
            .add_systems(Update, update_hud);
    }
}

/// Seconds without being hit before health starts coming back...
const REGEN_DELAY: f32 = 4.0;
/// ...at this many points per second.
const REGEN_RATE: f32 = 10.0;
/// Seconds the red flash lasts after a hit.
const HURT_FLASH: f32 = 0.35;
/// Below this share of maximum health the number turns red.
const LOW: f32 = 0.35;

/// Damage to the player, from an enemy shot (or anything else).
#[derive(Message, Clone, Copy)]
pub struct PlayerHit {
    pub damage: f32,
}

/// The player's health ran out. Sent once per death.
#[derive(Message, Clone, Copy)]
pub struct PlayerDied;

#[derive(Resource)]
pub struct PlayerHealth {
    pub current: f32,
    pub max: f32,
    /// Out of health and waiting to be revived (or for a new run).
    dead: bool,
    /// Seconds since the last hit.
    since_hit: f32,
    /// Seconds of red flash left.
    hurt: f32,
}

impl Default for PlayerHealth {
    fn default() -> Self {
        Self {
            current: 100.0,
            max: 100.0,
            dead: false,
            since_hit: 0.0,
            hurt: 0.0,
        }
    }
}

impl PlayerHealth {
    /// Lose health without being hit (flatlining): no flash or sound, but no
    /// healing either while it goes on.
    pub fn bleed(&mut self, amount: f32) {
        self.current -= amount;
        self.since_hit = 0.0;
    }

    /// Heal a little, if it's been long enough since the last hit or bleed.
    fn regen(&mut self, dt: f32) {
        if self.since_hit > REGEN_DELAY {
            self.current = (self.current + REGEN_RATE * dt).min(self.max);
        }
    }

    /// Back on your feet with `health`.
    pub fn revive(&mut self, health: f32) {
        self.current = health.min(self.max);
        self.dead = false;
        self.since_hit = 0.0;
    }
}

fn take_hits(
    mut commands: Commands,
    time: Res<Time>,
    sounds: Option<Res<Sounds>>,
    mut health: ResMut<PlayerHealth>,
    mut hits: MessageReader<PlayerHit>,
    mut died: MessageWriter<PlayerDied>,
) {
    let dt = time.delta_secs();
    health.since_hit += dt;
    let damage: f32 = hits.read().map(|hit| hit.damage).sum();
    if health.dead {
        return;
    }
    if damage > 0.0 {
        health.current -= damage;
        health.since_hit = 0.0;
        health.hurt = HURT_FLASH;
        if let Some(sounds) = &sounds {
            sfx::play(&mut commands, &sounds.hurt);
        }
    }
    if health.current <= 0.0 {
        health.current = 0.0;
        health.dead = true;
        died.write(PlayerDied);
    } else {
        health.regen(dt);
    }
}

#[derive(Component)]
struct HealthText;

#[derive(Component)]
struct HurtFlash;

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        HealthText,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(30.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            left: px(28),
            bottom: px(54),
            ..default()
        },
    ));
    // A red wash over the whole screen when hit.
    commands.spawn((
        HurtFlash,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundColor(Color::NONE),
    ));
}

fn update_hud(
    time: Res<Time>,
    mut health: ResMut<PlayerHealth>,
    text: Single<(&mut Text, &mut TextColor), With<HealthText>>,
    mut flash: Single<&mut BackgroundColor, With<HurtFlash>>,
) {
    health.hurt = (health.hurt - time.delta_secs()).max(0.0);
    let (mut text, mut colour) = text.into_inner();
    text.0 = format!(
        "HP {:3.0}/{:.0}",
        health.current.max(0.0).ceil(),
        health.max
    );
    colour.0 = if health.current < health.max * LOW {
        Color::srgb(0.95, 0.2, 0.15)
    } else {
        Color::srgb(0.9, 0.85, 0.8)
    };
    flash.0 = Color::srgba(0.7, 0.0, 0.0, (health.hurt / HURT_FLASH) * 0.45);
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    /// One tick as the game runs it: time passes, then healing.
    fn tick(health: &mut PlayerHealth) {
        health.since_hit += DT;
        health.regen(DT);
    }

    #[test]
    fn bleeding_drains_steadily_and_healing_never_cancels_it() {
        let mut health = PlayerHealth::default();
        // Ten seconds of flatlining at 4 a second, well past the regen delay.
        for _ in 0..600 {
            health.bleed(4.0 * DT);
            tick(&mut health);
        }
        assert!((health.current - 60.0).abs() < 0.1, "{}", health.current);
    }

    #[test]
    fn healing_comes_back_after_the_delay() {
        let mut health = PlayerHealth::default();
        health.bleed(50.0);
        // Not yet...
        for _ in 0..(REGEN_DELAY / DT) as usize - 1 {
            tick(&mut health);
        }
        assert!((health.current - 50.0).abs() < 1e-3, "{}", health.current);
        // ...then a point every tenth of a second.
        for _ in 0..61 {
            tick(&mut health);
        }
        assert!(
            health.current > 59.0 && health.current <= 61.0,
            "{}",
            health.current
        );
    }
}
