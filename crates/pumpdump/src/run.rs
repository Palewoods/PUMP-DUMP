//! Runs: pick a heart rate (and, at the fastest, a perk), play until you die,
//! then pick again.
//!
//! While a menu is up the game world is paused: virtual time stops, so the
//! fixed tick (movement, weapons, enemies) doesn't run at all.
//!
//! Pick with 1-5 (or 1-3 for perks), the mouse, or the arrow keys / D-pad and
//! Enter / A. `PUMPDUMP_AUTOSTART=5` (or `=5,2` for a perk too) skips the menu,
//! for testing.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::enemies::ResetEnemies;
use crate::health::{PlayerDied, PlayerHealth};
use crate::heart::{Boosts, Heart, HeartAsset, HeartHandle, HeartTuning, Perk, Run};
use crate::player::RespawnPlayer;
use crate::sfx::{self, Sounds};
use crate::weapon::Arsenal;

pub struct RunPlugin;

impl Plugin for RunPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Menu>()
            .add_message::<MenuPick>()
            .add_systems(Startup, (pause_for_menu, spawn_menu))
            .add_systems(
                Update,
                (autostart, menu_input, apply_pick, on_death, build_menu).chain(),
            );
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    /// Playing.
    Hidden,
    HeartRate {
        died: bool,
    },
    Perk,
}

#[derive(Resource)]
struct Menu {
    screen: Screen,
    /// The highlighted choice (keyboard / D-pad).
    selected: usize,
    /// What the menu's widgets were last built for (with how many choices),
    /// so they're only rebuilt when that changes.
    built: Option<(Screen, usize)>,
    /// The heart rate picked before going on to the perk screen.
    tier: usize,
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            screen: Screen::HeartRate { died: false },
            selected: 0,
            built: None,
            tier: 0,
        }
    }
}

/// A choice was made on the current screen (index into its choices).
#[derive(Message, Clone, Copy)]
struct MenuPick(usize);

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct MenuSubtitle;

#[derive(Component)]
struct MenuChoices;

/// A choice button: its index on the current screen.
#[derive(Component)]
struct Choice(usize);

fn pause_for_menu(mut time: ResMut<Time<Virtual>>) {
    time.pause();
}

fn spawn_menu(mut commands: Commands) {
    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(14),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.0, 0.0, 0.88)),
            // Above all the HUD.
            GlobalZIndex(20),
        ))
        .with_children(|menu| {
            menu.spawn((
                Text::new("PUMP&DUMP"),
                TextFont {
                    font_size: FontSize::Px(56.0),
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.1, 0.1)),
            ));
            menu.spawn((
                MenuSubtitle,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(24.0),
                    ..default()
                },
                TextColor(Color::srgb(0.95, 0.85, 0.75)),
            ));
            menu.spawn((
                MenuChoices,
                Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    justify_content: JustifyContent::Center,
                    column_gap: px(12),
                    row_gap: px(12),
                    ..default()
                },
            ));
            menu.spawn((
                Text::new("Pick with the number keys, the mouse, or arrows / D-pad and Enter / A"),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(Color::srgb(0.6, 0.55, 0.5)),
            ));
        });
}

/// The label for each choice on a screen.
fn choices(screen: Screen, tuning: &HeartTuning) -> Vec<String> {
    match screen {
        Screen::Hidden => Vec::new(),
        Screen::HeartRate { .. } => {
            let last = tuning.tiers.len().saturating_sub(1);
            tuning
                .tiers
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let perk = if i == last { "\n\n+ PICK A PERK" } else { "" };
                    format!(
                        "{}  {}\n{} BPM\nlasts {:.0} s per fill\n\nDAMAGE x{:.2}\nACCURACY +{:.0}%\nHEALTH {:.0}\nSPEED x{:.2}\nRELOAD {:.0}% faster{perk}",
                        i + 1,
                        t.name,
                        t.bpm,
                        t.lasts,
                        t.damage,
                        (1.0 - t.spread) * 100.0,
                        t.health,
                        t.speed,
                        (1.0 - t.reload) * 100.0,
                    )
                })
                .collect()
        }
        Screen::Perk => Perk::ALL
            .iter()
            .enumerate()
            .map(|(i, perk)| format!("{}  {}\n\n{}", i + 1, perk.name(), perk.description(tuning)))
            .collect(),
    }
}

/// Keep the menu's widgets in step with the screen: rebuild the choice buttons
/// when the screen changes, and highlight the selected / hovered one.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn build_menu(
    mut commands: Commands,
    mut menu: ResMut<Menu>,
    handle: Res<HeartHandle>,
    tunings: Res<Assets<HeartAsset>>,
    mut root: Single<&mut Visibility, With<MenuRoot>>,
    mut subtitle: Single<&mut Text, With<MenuSubtitle>>,
    container: Single<Entity, With<MenuChoices>>,
    mut buttons: Query<(
        &Choice,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    **root = if menu.screen == Screen::Hidden {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    let Some(tuning) = tunings.get(&handle.0) else {
        subtitle.0 = "...".to_string();
        return;
    };
    let labels = choices(menu.screen, tuning);
    if menu.built != Some((menu.screen, labels.len())) {
        menu.built = Some((menu.screen, labels.len()));
        subtitle.0 = match menu.screen {
            Screen::HeartRate { died: true } => {
                "YOU DIED.  Pick a heart rate for the next run.".to_string()
            }
            Screen::HeartRate { died: false } => "You're a zombie: your heart only beats when you squeeze it.\nPick how fast it beats. Faster means more squeezing, and more power.".to_string(),
            Screen::Perk => "A racing heart. Pick a perk.".to_string(),
            Screen::Hidden => String::new(),
        };
        commands.entity(*container).despawn_children();
        commands.entity(*container).with_children(|row| {
            for (i, label) in labels.iter().enumerate() {
                row.spawn((
                    Choice(i),
                    Button,
                    Node {
                        width: px(200),
                        padding: UiRect::all(px(12)),
                        border: UiRect::all(px(2)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    BorderColor::all(Color::NONE),
                ))
                .with_child((
                    Text::new(label.clone()),
                    TextFont {
                        font_size: FontSize::Px(16.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.95, 0.9, 0.85)),
                ));
            }
        });
        return;
    }
    for (choice, interaction, mut background, mut border) in &mut buttons {
        let lit = choice.0 == menu.selected || *interaction != Interaction::None;
        // Faster heart rates glow redder.
        let heat = choice.0 as f32 / labels.len().max(1) as f32;
        background.0 = if lit {
            Color::srgba(0.45 + 0.3 * heat, 0.08, 0.06, 0.9)
        } else {
            Color::srgba(0.2 + 0.2 * heat, 0.05, 0.04, 0.8)
        };
        *border = BorderColor::all(if lit {
            Color::srgb(1.0, 0.75, 0.5)
        } else {
            Color::srgb(0.35, 0.1, 0.08)
        });
    }
}

/// Turn keys, clicks and the D-pad into picks.
fn menu_input(
    mut menu: ResMut<Menu>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    buttons: Query<(&Choice, &Interaction), Changed<Interaction>>,
    mut picks: MessageWriter<MenuPick>,
) {
    let count = match menu.built {
        Some((screen, count)) if screen == menu.screen && screen != Screen::Hidden => count,
        _ => return,
    };
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
    ];
    for (i, key) in digits.iter().enumerate().take(count) {
        if keys.just_pressed(*key) {
            picks.write(MenuPick(i));
            return;
        }
    }
    for (choice, interaction) in &buttons {
        match interaction {
            Interaction::Pressed => {
                picks.write(MenuPick(choice.0));
                return;
            }
            Interaction::Hovered => menu.selected = choice.0,
            Interaction::None => {}
        }
    }
    let pad = |button| gamepads.iter().any(|g| g.just_pressed(button));
    if keys.just_pressed(KeyCode::ArrowRight) || pad(GamepadButton::DPadRight) {
        menu.selected = (menu.selected + 1) % count;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft) {
        menu.selected = (menu.selected + count - 1) % count;
    }
    menu.selected = menu.selected.min(count - 1);
    if keys.just_pressed(KeyCode::Enter) || pad(GamepadButton::South) {
        picks.write(MenuPick(menu.selected));
    }
}

/// `PUMPDUMP_AUTOSTART=tier[,perk]` (1-based) picks for you, for testing.
fn autostart(menu: Res<Menu>, mut picks: MessageWriter<MenuPick>, mut done: Local<u8>) {
    let Ok(choice) = std::env::var("PUMPDUMP_AUTOSTART") else {
        return;
    };
    let mut parts = choice
        .split(',')
        .map(|p| p.trim().parse::<usize>().unwrap_or(1).max(1) - 1);
    let ready = matches!(menu.built, Some((screen, _)) if screen == menu.screen);
    match (menu.screen, *done) {
        (Screen::HeartRate { died: false }, 0) if ready => {
            picks.write(MenuPick(parts.next().unwrap_or(0)));
            *done = 1;
        }
        // Only if a perk was given: otherwise stay on the perk screen.
        (Screen::Perk, 1) if ready => {
            if let Some(perk) = parts.nth(1) {
                picks.write(MenuPick(perk));
            }
            *done = 2;
        }
        _ => {}
    }
}

/// Act on a pick: go on to the perk screen, or start the run.
#[allow(clippy::too_many_arguments)]
fn apply_pick(
    mut picks: MessageReader<MenuPick>,
    mut menu: ResMut<Menu>,
    handle: Res<HeartHandle>,
    tunings: Res<Assets<HeartAsset>>,
    mut run: ResMut<Run>,
    mut boosts: ResMut<Boosts>,
    mut heart: ResMut<Heart>,
    mut health: ResMut<PlayerHealth>,
    mut arsenal: ResMut<Arsenal>,
    mut time: ResMut<Time<Virtual>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mut respawn: MessageWriter<RespawnPlayer>,
    mut reset: MessageWriter<ResetEnemies>,
) {
    let Some(MenuPick(choice)) = picks.read().last().copied() else {
        return;
    };
    let Some(tuning) = tunings.get(&handle.0) else {
        return;
    };
    let perk = match menu.screen {
        Screen::Hidden => return,
        Screen::HeartRate { .. } => {
            menu.tier = choice.min(tuning.tiers.len().saturating_sub(1));
            if menu.tier + 1 == tuning.tiers.len() {
                menu.screen = Screen::Perk;
                menu.selected = 0;
                return;
            }
            None
        }
        Screen::Perk => Some(Perk::ALL[choice.min(Perk::ALL.len() - 1)]),
    };
    let Some(tier) = tuning.tier(menu.tier) else {
        return;
    };

    *run = Run {
        started: true,
        tier: menu.tier,
        perk,
        second_heart_used: false,
    };
    *boosts = Boosts::from_tier(tier);
    *heart = Heart {
        bpm: tier.bpm as f32,
        ..default()
    };
    health.max = boosts.max_health;
    health.revive(boosts.max_health);
    *arsenal = Arsenal::default();
    respawn.write(RespawnPlayer);
    reset.write(ResetEnemies);
    menu.screen = Screen::Hidden;
    time.unpause();
    // Straight into it: capture the mouse for looking.
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
}

/// Death ends the run (unless the Second Heart perk saves you): back to the
/// heart rate screen.
#[allow(clippy::too_many_arguments)]
fn on_death(
    mut commands: Commands,
    mut deaths: MessageReader<PlayerDied>,
    mut menu: ResMut<Menu>,
    mut run: ResMut<Run>,
    handle: Res<HeartHandle>,
    tunings: Res<Assets<HeartAsset>>,
    sounds: Option<Res<Sounds>>,
    mut heart: ResMut<Heart>,
    mut health: ResMut<PlayerHealth>,
    mut time: ResMut<Time<Virtual>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if deaths.read().count() == 0 || !run.started {
        return;
    }
    if run.perk == Some(Perk::SecondHeart) && !run.second_heart_used {
        let share = tunings
            .get(&handle.0)
            .map_or(0.5, |t| t.second_heart_health);
        run.second_heart_used = true;
        let max = health.max;
        health.revive(max * share);
        heart.blood = heart.blood.max(0.5);
        if let Some(sounds) = &sounds {
            sfx::play(&mut commands, &sounds.heartbeat);
        }
        return;
    }
    run.started = false;
    heart.held = false;
    menu.screen = Screen::HeartRate { died: true };
    menu.selected = run.tier;
    time.pause();
    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tier_and_perk_gets_a_choice() {
        let tuning: HeartTuning = ron::from_str(include_str!("../assets/heart.ron")).unwrap();
        let tiers = choices(Screen::HeartRate { died: false }, &tuning);
        assert_eq!(tiers.len(), tuning.tiers.len());
        assert!(tiers.last().unwrap().contains("PERK"));
        assert!(!tiers[0].contains("PERK"));
        assert_eq!(choices(Screen::Perk, &tuning).len(), Perk::ALL.len());
        assert!(choices(Screen::Hidden, &tuning).is_empty());
    }
}
