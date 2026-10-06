//! The menus and the flow of a game: the title screen, picking a level, a
//! heart rate (and, at the fastest, a perk), playing, pausing, dying, and
//! clearing a level.
//!
//! While a menu is up the game world is paused: virtual time stops, so the
//! fixed tick (movement, weapons, enemies) doesn't run at all. Behind the title
//! screen the view slowly turns round the level.
//!
//! Pick with the number keys, the mouse, or the arrow keys / D-pad and
//! Enter / A. Esc / B goes back; while playing, Esc / Start pauses.
//! `PUMPDUMP_AUTOSTART=tier[,perk]` (1-based) skips the menus, for testing,
//! into level `PUMPDUMP_LEVEL` (default 1; 0 is training).

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::health::{PlayerDied, PlayerHealth};
use crate::heart::{Boosts, Heart, HeartAsset, HeartHandle, HeartTuning, Perk, Run};
use crate::levels::{
    self, COUNT, CurrentLevel, LevelCleared, LevelLoad, LevelStats, LoadLevel, TRAINING,
};
use crate::player::{CONTROLS, Showcase};
use crate::sfx::{self, Sounds};
use crate::weapon::Arsenal;

pub struct RunPlugin;

impl Plugin for RunPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Menu>()
            .add_message::<MenuPick>()
            .add_systems(Startup, (pause_for_menu, spawn_menu, load_backdrop))
            .add_systems(
                Update,
                (
                    autostart, escape, menu_input, apply_pick, on_death, on_cleared, build_menu,
                    show_hud,
                )
                    .chain()
                    // Level loads asked for here happen this same frame.
                    .before(LevelLoad),
            );
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    /// Playing.
    Hidden,
    Title,
    Levels,
    Controls,
    HeartRate,
    Perk,
    Paused,
    Died,
    Cleared,
}

impl Screen {
    /// Over a level being played: the HUD stays up behind it.
    fn in_game(self) -> bool {
        matches!(
            self,
            Screen::Hidden | Screen::Paused | Screen::Died | Screen::Cleared
        )
    }

    /// Big cards side by side, rather than a list of buttons.
    fn cards(self) -> bool {
        matches!(self, Screen::Levels | Screen::HeartRate | Screen::Perk)
    }

    /// Where Esc / B goes from here.
    fn back(self) -> Option<Screen> {
        match self {
            Screen::Levels | Screen::Controls | Screen::HeartRate => Some(Screen::Title),
            Screen::Perk => Some(Screen::HeartRate),
            _ => None,
        }
    }
}

#[derive(Resource)]
struct Menu {
    screen: Screen,
    /// The highlighted choice (keyboard / D-pad).
    selected: usize,
    /// What the menu's widgets were last built for (with how many choices),
    /// so they're only rebuilt when that changes.
    built: Option<(Screen, usize)>,
    /// The level being played (or about to be).
    level: usize,
    /// The heart rate and perk picked, kept for retries and the next level.
    tier: usize,
    perk: Option<Perk>,
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            screen: Screen::Title,
            selected: 0,
            built: None,
            level: 1,
            tier: 0,
            perk: None,
        }
    }
}

/// A choice was made on the current screen (index into its choices).
#[derive(Message, Clone, Copy)]
struct MenuPick(usize);

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct MenuTitle;

#[derive(Component)]
struct MenuSubtitle;

#[derive(Component)]
struct MenuChoices;

#[derive(Component)]
struct MenuFooter;

/// A choice button: its index on the current screen.
#[derive(Component)]
struct Choice(usize);

const TITLE_SIZE: f32 = 56.0;

fn pause_for_menu(mut time: ResMut<Time<Virtual>>) {
    time.pause();
}

/// Something to look at behind the title screen.
fn load_backdrop(mut loads: MessageWriter<LoadLevel>) {
    loads.write(LoadLevel(1));
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
                MenuTitle,
                Text::new("PUMP&DUMP"),
                TextFont {
                    font_size: FontSize::Px(TITLE_SIZE),
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
                TextLayout::justify(Justify::Center),
            ));
            menu.spawn((
                MenuChoices,
                Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    column_gap: px(12),
                    row_gap: px(10),
                    ..default()
                },
            ));
            menu.spawn((
                MenuFooter,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(Color::srgb(0.6, 0.55, 0.5)),
                TextLayout::justify(Justify::Center),
            ));
        });
}

/// The label for each choice on a screen.
fn choices(screen: Screen, tuning: &HeartTuning, current: &CurrentLevel) -> Vec<String> {
    let list = |items: &[&str]| items.iter().map(|s| s.to_string()).collect();
    match screen {
        Screen::Hidden => Vec::new(),
        Screen::Title => list(&["PLAY", "LEVEL SELECT", "TRAINING", "CONTROLS", "QUIT"]),
        Screen::Levels => (1..COUNT)
            .map(|i| {
                let (name, blurb) = levels::info(i);
                let hunters = levels::level(i).hunters.len();
                format!("{i}  {name}\n\n{blurb}\n\n{hunters} HUNTERS")
            })
            .collect(),
        Screen::Controls => list(&["BACK"]),
        Screen::HeartRate => {
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
        Screen::Paused => list(&["RESUME", "RESTART LEVEL", "MAIN MENU"]),
        Screen::Died => list(&["RETRY", "NEW HEART RATE", "MAIN MENU"]),
        Screen::Cleared => match next_level(current.index) {
            Some(next) => vec![
                format!("NEXT: {}", levels::info(next).0),
                "REPLAY".to_string(),
                "MAIN MENU".to_string(),
            ],
            None => list(&["MAIN MENU", "REPLAY"]),
        },
    }
}

/// The level after `index` in the campaign, if there is one.
fn next_level(index: usize) -> Option<usize> {
    (index != TRAINING && index + 1 < COUNT).then_some(index + 1)
}

/// The line (or lines) under the title.
fn subtitle(screen: Screen, current: &CurrentLevel, stats: &LevelStats) -> String {
    let (name, _) = levels::info(current.index);
    let record = format!(
        "Time {}    Kills {}",
        levels::clock(stats.time),
        stats.kills
    );
    match screen {
        Screen::Hidden => String::new(),
        Screen::Title => "You're dead. Your heart won't beat by itself, so you squeeze it.\nThen you go and hunt the hunters.".to_string(),
        Screen::Levels => "Pick a level.".to_string(),
        Screen::Controls | Screen::Paused => CONTROLS.to_string(),
        Screen::HeartRate => "Pick how fast your heart beats.\nFaster means more squeezing, and more power.".to_string(),
        Screen::Perk => "A racing heart. Pick a perk.".to_string(),
        Screen::Died => format!("{name}\n\n{record}"),
        Screen::Cleared => match next_level(current.index) {
            Some(_) => format!("{name}\n\n{record}"),
            None => format!("{name}\n\n{record}\n\nYou made it out. For now."),
        },
    }
}

/// The big heading for each screen.
fn heading(screen: Screen) -> &'static str {
    match screen {
        Screen::Paused => "PAUSED",
        Screen::Died => "YOU DIED",
        Screen::Cleared => "LEVEL CLEARED",
        Screen::Controls => "CONTROLS",
        Screen::Levels => "LEVELS",
        _ => "PUMP&DUMP",
    }
}

/// Keep the menu's widgets in step with the screen: rebuild the choice buttons
/// when the screen changes, and highlight the selected / hovered one.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn build_menu(
    mut commands: Commands,
    mut menu: ResMut<Menu>,
    real: Res<Time<Real>>,
    handle: Res<HeartHandle>,
    tunings: Res<Assets<HeartAsset>>,
    current: Res<CurrentLevel>,
    stats: Res<LevelStats>,
    root: Single<(&mut Visibility, &mut BackgroundColor), (With<MenuRoot>, Without<Choice>)>,
    title: Single<(&mut Text, &mut TextFont), (With<MenuTitle>, Without<MenuSubtitle>)>,
    subtitle_text: Single<
        (&mut Text, &mut TextFont),
        (With<MenuSubtitle>, Without<MenuTitle>, Without<MenuFooter>),
    >,
    mut footer: Single<&mut Text, (With<MenuFooter>, Without<MenuTitle>, Without<MenuSubtitle>)>,
    container: Single<(Entity, &mut Node), With<MenuChoices>>,
    mut buttons: Query<
        (
            &Choice,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Without<MenuRoot>,
    >,
) {
    let screen = menu.screen;
    let (mut shown, mut backdrop) = root.into_inner();
    *shown = if screen == Screen::Hidden {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    // The title throbs like a heartbeat, at 60 BPM.
    let (mut title, mut title_font) = title.into_inner();
    let beat = real.elapsed_secs().fract();
    let throb = if screen == Screen::Title {
        (1.0 - beat * 5.0).max(0.0) + 0.5 * (1.0 - (beat - 0.2).abs() * 8.0).max(0.0)
    } else {
        0.0
    };
    title_font.font_size = FontSize::Px(TITLE_SIZE * (1.0 + 0.08 * throb));

    let Some(tuning) = tunings.get(&handle.0) else {
        return;
    };
    let labels = choices(screen, tuning, &current);
    if menu.built != Some((screen, labels.len())) {
        menu.built = Some((screen, labels.len()));
        title.0 = heading(screen).to_string();
        let (mut subtitle_line, mut subtitle_font) = subtitle_text.into_inner();
        subtitle_line.0 = subtitle(screen, &current, &stats);
        // The controls are a lot of text: smaller.
        let small = matches!(screen, Screen::Controls | Screen::Paused);
        subtitle_font.font_size = FontSize::Px(if small { 15.0 } else { 24.0 });
        footer.0 = if screen == Screen::Title {
            format!(
                "v{}   Arrows / D-pad and Enter / A, the number keys, or the mouse",
                env!("CARGO_PKG_VERSION")
            )
        } else if screen.back().is_some() {
            "Arrows / D-pad and Enter / A, the number keys, or the mouse.   Esc / B: back"
                .to_string()
        } else {
            "Arrows / D-pad and Enter / A, the number keys, or the mouse".to_string()
        };
        // See the level slowly turning behind the title screen.
        backdrop.0 = Color::srgba(
            0.03,
            0.0,
            0.0,
            if screen == Screen::Title { 0.45 } else { 0.88 },
        );

        let (container, mut layout) = container.into_inner();
        (layout.flex_direction, layout.align_items) = if screen.cards() {
            (FlexDirection::Row, AlignItems::Stretch)
        } else {
            (FlexDirection::Column, AlignItems::Center)
        };
        commands.entity(container).despawn_children();
        commands.entity(container).with_children(|row| {
            for (i, label) in labels.iter().enumerate() {
                let (width, justify) = if screen.cards() {
                    (
                        px(if screen == Screen::Levels { 260 } else { 200 }),
                        Justify::Left,
                    )
                } else {
                    (px(340), Justify::Center)
                };
                row.spawn((
                    Choice(i),
                    Button,
                    Node {
                        width,
                        padding: UiRect::all(px(12)),
                        border: UiRect::all(px(2)),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    BorderColor::all(Color::NONE),
                ))
                .with_child((
                    Text::new(label.clone()),
                    TextFont {
                        font_size: FontSize::Px(if screen.cards() { 16.0 } else { 22.0 }),
                        ..default()
                    },
                    TextColor(Color::srgb(0.95, 0.9, 0.85)),
                    TextLayout::justify(justify),
                ));
            }
        });
        return;
    }
    for (choice, interaction, mut background, mut border) in &mut buttons {
        let lit = choice.0 == menu.selected || *interaction != Interaction::None;
        // Faster heart rates (and later levels) glow redder.
        let heat = if screen.cards() {
            choice.0 as f32 / labels.len().max(1) as f32
        } else {
            0.2
        };
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

/// Free the mouse for the menus (and pause the world), or capture it for play.
fn hold_still(time: &mut Time<Virtual>, cursor: &mut CursorOptions) {
    time.pause();
    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
}

fn carry_on(time: &mut Time<Virtual>, cursor: &mut CursorOptions) {
    time.unpause();
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
}

/// Esc / Start pauses and unpauses; Esc / B goes back a screen.
fn escape(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    run: Res<Run>,
    mut menu: ResMut<Menu>,
    mut time: ResMut<Time<Virtual>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let pad = |button| gamepads.iter().any(|g| g.just_pressed(button));
    let escape = keys.just_pressed(KeyCode::Escape);
    let start = pad(GamepadButton::Start);
    match menu.screen {
        Screen::Hidden if run.started && (escape || start) => {
            menu.screen = Screen::Paused;
            menu.selected = 0;
            hold_still(&mut time, &mut cursor);
        }
        Screen::Paused if escape || start || pad(GamepadButton::East) => {
            menu.screen = Screen::Hidden;
            carry_on(&mut time, &mut cursor);
        }
        screen => {
            if let Some(back) = screen.back()
                && (escape || pad(GamepadButton::East))
            {
                menu.screen = back;
                menu.selected = 0;
            }
        }
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
    let next = keys.any_just_pressed([KeyCode::ArrowRight, KeyCode::ArrowDown])
        || pad(GamepadButton::DPadRight)
        || pad(GamepadButton::DPadDown);
    let previous = keys.any_just_pressed([KeyCode::ArrowLeft, KeyCode::ArrowUp])
        || pad(GamepadButton::DPadLeft)
        || pad(GamepadButton::DPadUp);
    if next {
        menu.selected = (menu.selected + 1) % count;
    }
    if previous {
        menu.selected = (menu.selected + count - 1) % count;
    }
    menu.selected = menu.selected.min(count - 1);
    if keys.just_pressed(KeyCode::Enter) || pad(GamepadButton::South) {
        picks.write(MenuPick(menu.selected));
    }
}

/// `PUMPDUMP_AUTOSTART=tier[,perk]` (1-based) picks for you, for testing, in
/// level `PUMPDUMP_LEVEL` (default 1).
fn autostart(
    mut menu: ResMut<Menu>,
    mut picks: MessageWriter<MenuPick>,
    mut loads: MessageWriter<LoadLevel>,
    mut done: Local<u8>,
) {
    let Ok(choice) = std::env::var("PUMPDUMP_AUTOSTART") else {
        return;
    };
    let mut parts = choice
        .split(',')
        .map(|p| p.trim().parse::<usize>().unwrap_or(1).max(1) - 1);
    let ready = matches!(menu.built, Some((screen, _)) if screen == menu.screen);
    match (menu.screen, *done) {
        (Screen::Title, 0) if ready => {
            menu.level = std::env::var("PUMPDUMP_LEVEL")
                .ok()
                .and_then(|l| l.trim().parse().ok())
                .unwrap_or(1usize)
                .min(COUNT - 1);
            loads.write(LoadLevel(menu.level));
            menu.screen = Screen::HeartRate;
            *done = 1;
        }
        (Screen::HeartRate, 1) if ready => {
            picks.write(MenuPick(parts.next().unwrap_or(0)));
            *done = 2;
        }
        // Only if a perk was given: otherwise stay on the perk screen.
        (Screen::Perk, 2) if ready => {
            if let Some(perk) = parts.nth(1) {
                picks.write(MenuPick(perk));
            }
            *done = 3;
        }
        _ => {}
    }
}

/// Act on a pick: move between screens, start or restart a level, quit.
#[allow(clippy::too_many_arguments)]
fn apply_pick(
    mut picks: MessageReader<MenuPick>,
    mut menu: ResMut<Menu>,
    handle: Res<HeartHandle>,
    tunings: Res<Assets<HeartAsset>>,
    current: Res<CurrentLevel>,
    mut run: ResMut<Run>,
    mut boosts: ResMut<Boosts>,
    mut heart: ResMut<Heart>,
    mut health: ResMut<PlayerHealth>,
    mut arsenal: ResMut<Arsenal>,
    mut time: ResMut<Time<Virtual>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mut loads: MessageWriter<LoadLevel>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(MenuPick(choice)) = picks.read().last().copied() else {
        return;
    };
    let Some(tuning) = tunings.get(&handle.0) else {
        return;
    };
    fn go(menu: &mut Menu, screen: Screen) {
        menu.screen = screen;
        // The heart rate screen starts on the last one picked.
        menu.selected = if screen == Screen::HeartRate {
            menu.tier
        } else {
            0
        };
    }
    // Pick a level: show it behind the menus, then on to the heart rate.
    let mut pick_level = |menu: &mut Menu, level: usize| {
        menu.level = level;
        loads.write(LoadLevel(level));
        go(menu, Screen::HeartRate);
    };
    enum Then {
        Stay,
        Play,
        MainMenu,
    }
    let then = match menu.screen {
        Screen::Hidden => Then::Stay,
        Screen::Title => {
            match choice {
                0 => pick_level(&mut menu, 1),
                1 => go(&mut menu, Screen::Levels),
                2 => pick_level(&mut menu, TRAINING),
                3 => go(&mut menu, Screen::Controls),
                _ => {
                    exit.write(AppExit::Success);
                }
            }
            Then::Stay
        }
        Screen::Levels => {
            pick_level(&mut menu, (choice + 1).min(COUNT - 1));
            Then::Stay
        }
        Screen::Controls => {
            go(&mut menu, Screen::Title);
            Then::Stay
        }
        Screen::HeartRate => {
            menu.tier = choice.min(tuning.tiers.len().saturating_sub(1));
            menu.perk = None;
            if menu.tier + 1 == tuning.tiers.len() {
                go(&mut menu, Screen::Perk);
                Then::Stay
            } else {
                Then::Play
            }
        }
        Screen::Perk => {
            menu.perk = Some(Perk::ALL[choice.min(Perk::ALL.len() - 1)]);
            Then::Play
        }
        Screen::Paused => match choice {
            0 => {
                menu.screen = Screen::Hidden;
                carry_on(&mut time, &mut cursor);
                Then::Stay
            }
            1 => Then::Play,
            _ => Then::MainMenu,
        },
        Screen::Died => match choice {
            0 => Then::Play,
            1 => {
                go(&mut menu, Screen::HeartRate);
                Then::Stay
            }
            _ => Then::MainMenu,
        },
        Screen::Cleared => match (next_level(current.index), choice) {
            (Some(next), 0) => {
                menu.level = next;
                Then::Play
            }
            (Some(_), 1) | (None, 1) => Then::Play,
            _ => Then::MainMenu,
        },
    };

    match then {
        Then::Stay => {}
        Then::MainMenu => {
            run.started = false;
            heart.held = false;
            go(&mut menu, Screen::Title);
            // Fresh, for looking at.
            loads.write(LoadLevel(current.index));
            hold_still(&mut time, &mut cursor);
        }
        Then::Play => {
            let Some(tier) = tuning.tier(menu.tier) else {
                return;
            };
            *run = Run {
                started: true,
                tier: menu.tier,
                perk: menu.perk,
                second_heart_used: false,
                boosts: Boosts::from_tier(tier),
            };
            *boosts = run.boosts;
            *heart = Heart {
                bpm: tier.bpm as f32,
                ..default()
            };
            health.max = boosts.max_health;
            health.revive(boosts.max_health);
            *arsenal = Arsenal::default();
            loads.write(LoadLevel(menu.level));
            menu.screen = Screen::Hidden;
            carry_on(&mut time, &mut cursor);
        }
    }
}

/// Death ends the run (unless the Second Heart perk saves you).
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
    menu.screen = Screen::Died;
    menu.selected = 0;
    hold_still(&mut time, &mut cursor);
}

/// Reached the exit: the level-clear screen.
#[allow(clippy::too_many_arguments)]
fn on_cleared(
    mut commands: Commands,
    mut cleared: MessageReader<LevelCleared>,
    mut menu: ResMut<Menu>,
    mut run: ResMut<Run>,
    sounds: Option<Res<Sounds>>,
    mut heart: ResMut<Heart>,
    mut time: ResMut<Time<Virtual>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if cleared.read().count() == 0 || !run.started {
        return;
    }
    run.started = false;
    heart.held = false;
    menu.screen = Screen::Cleared;
    menu.selected = 0;
    if let Some(sounds) = &sounds {
        sfx::play(&mut commands, &sounds.heartbeat);
    }
    hold_still(&mut time, &mut cursor);
}

/// Hide the HUD on the menus before a level starts, and let the view turn by
/// itself behind them.
// Bevy note: the HUD's pieces are the top-level UI nodes without a
// `GlobalZIndex`; the retro screen (drawn under everything) and the menu
// (over everything) both have one.
#[allow(clippy::type_complexity)]
fn show_hud(
    menu: Res<Menu>,
    mut showcase: ResMut<Showcase>,
    mut huds: Query<&mut Visibility, (With<Node>, Without<ChildOf>, Without<GlobalZIndex>)>,
) {
    let in_game = menu.screen.in_game();
    showcase.0 = !in_game;
    let wanted = if in_game {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut huds {
        visibility.set_if_neq(wanted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> HeartTuning {
        ron::from_str(include_str!("../assets/heart.ron")).unwrap()
    }

    #[test]
    fn every_tier_perk_and_level_gets_a_choice() {
        let tuning = tuning();
        let current = CurrentLevel::default();
        let tiers = choices(Screen::HeartRate, &tuning, &current);
        assert_eq!(tiers.len(), tuning.tiers.len());
        assert!(tiers.last().unwrap().contains("PERK"));
        assert!(!tiers[0].contains("PERK"));
        assert_eq!(
            choices(Screen::Perk, &tuning, &current).len(),
            Perk::ALL.len()
        );
        // Every level but training.
        let levels = choices(Screen::Levels, &tuning, &current);
        assert_eq!(levels.len(), COUNT - 1);
        assert!(levels[0].contains(levels::info(1).0));
        assert!(choices(Screen::Hidden, &tuning, &current).is_empty());
    }

    #[test]
    fn the_campaign_runs_in_order_and_training_goes_nowhere() {
        assert_eq!(next_level(1), Some(2));
        assert_eq!(next_level(COUNT - 1), None);
        assert_eq!(next_level(TRAINING), None);
        // The last level's clear screen offers the way back, not a next level.
        let current = CurrentLevel {
            index: COUNT - 1,
            ..default()
        };
        let last = choices(Screen::Cleared, &tuning(), &current);
        assert_eq!(last[0], "MAIN MENU");
        let current = CurrentLevel {
            index: 1,
            ..default()
        };
        let first = choices(Screen::Cleared, &tuning(), &current);
        assert!(first[0].starts_with("NEXT"), "{first:?}");
    }

    #[test]
    fn back_goes_up_a_screen_and_only_from_the_menus() {
        assert_eq!(Screen::Perk.back(), Some(Screen::HeartRate));
        assert_eq!(Screen::HeartRate.back(), Some(Screen::Title));
        assert_eq!(Screen::Title.back(), None);
        assert_eq!(Screen::Died.back(), None);
        assert!(Screen::Paused.in_game() && !Screen::Title.in_game());
    }
}
