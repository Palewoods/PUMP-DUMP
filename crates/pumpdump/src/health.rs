//! The player's health: taking hits, a red flash, slowly healing when left
//! alone, and dying (back to the spawn with full health).

use bevy::prelude::*;

use crate::player::{MovementTick, RespawnPlayer};
use crate::sfx::{self, Sounds};

pub struct HealthPlugin;

impl Plugin for HealthPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerHealth>()
            .add_message::<PlayerHit>()
            .add_systems(Startup, spawn_hud)
            .add_systems(FixedUpdate, take_hits.after(MovementTick))
            .add_systems(Update, update_hud);
    }
}

const MAX_HEALTH: f32 = 100.0;
/// Seconds without being hit before health starts coming back...
const REGEN_DELAY: f32 = 4.0;
/// ...at this many points per second.
const REGEN_RATE: f32 = 10.0;
/// Seconds the red flash lasts after a hit.
const HURT_FLASH: f32 = 0.35;
/// Seconds "YOU DIED" stays up.
const DEATH_MESSAGE: f32 = 2.5;

/// Damage to the player, from an enemy shot (or anything else).
#[derive(Message, Clone, Copy)]
pub struct PlayerHit {
    pub damage: f32,
}

#[derive(Resource)]
pub struct PlayerHealth {
    pub current: f32,
    /// Seconds since the last hit.
    since_hit: f32,
    /// Seconds of red flash left.
    hurt: f32,
    /// Seconds of "YOU DIED" left.
    died: f32,
}

impl Default for PlayerHealth {
    fn default() -> Self {
        Self {
            current: MAX_HEALTH,
            since_hit: 0.0,
            hurt: 0.0,
            died: 0.0,
        }
    }
}

fn take_hits(
    mut commands: Commands,
    time: Res<Time>,
    sounds: Option<Res<Sounds>>,
    mut health: ResMut<PlayerHealth>,
    mut hits: MessageReader<PlayerHit>,
    mut respawn: MessageWriter<RespawnPlayer>,
) {
    let dt = time.delta_secs();
    health.since_hit += dt;
    let damage: f32 = hits.read().map(|hit| hit.damage).sum();
    if damage > 0.0 {
        health.current -= damage;
        health.since_hit = 0.0;
        health.hurt = HURT_FLASH;
        if let Some(sounds) = &sounds {
            sfx::play(&mut commands, &sounds.hurt);
        }
    }
    if health.current <= 0.0 {
        respawn.write(RespawnPlayer);
        health.current = MAX_HEALTH;
        health.died = DEATH_MESSAGE;
    } else if health.since_hit > REGEN_DELAY {
        health.current = (health.current + REGEN_RATE * dt).min(MAX_HEALTH);
    }
}

#[derive(Component)]
struct HealthText;

#[derive(Component)]
struct HurtFlash;

#[derive(Component)]
struct DeathText;

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
            DeathText,
            Text::new("YOU DIED"),
            TextFont {
                font_size: FontSize::Px(72.0),
                ..default()
            },
            TextColor(Color::srgb(0.75, 0.05, 0.03)),
            Visibility::Hidden,
        ));
}

fn update_hud(
    time: Res<Time>,
    mut health: ResMut<PlayerHealth>,
    text: Single<(&mut Text, &mut TextColor), With<HealthText>>,
    mut flash: Single<&mut BackgroundColor, With<HurtFlash>>,
    mut death: Single<&mut Visibility, With<DeathText>>,
) {
    let dt = time.delta_secs();
    health.hurt = (health.hurt - dt).max(0.0);
    health.died = (health.died - dt).max(0.0);

    let (mut text, mut colour) = text.into_inner();
    text.0 = format!("HP {:3.0}", health.current.max(0.0).ceil());
    colour.0 = if health.current < 35.0 {
        Color::srgb(0.95, 0.2, 0.15)
    } else {
        Color::srgb(0.9, 0.85, 0.8)
    };
    flash.0 = Color::srgba(0.7, 0.0, 0.0, (health.hurt / HURT_FLASH) * 0.45);
    **death = if health.died > 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
}
