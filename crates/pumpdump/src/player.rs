//! The player: feeds actions into `pumpdump_movement::step` every fixed tick, and places
//! the camera (first or third person) and the player's zombie body every rendered frame.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use leafwing_input_manager::prelude::*;
use pumpdump_movement::{CollisionWorld, Grapple, MoveInput, MovementState, WallKind, step};

use crate::character::{self, CharacterKit, Gait, Style};
use crate::heart::Boosts;
use crate::input::{self, Action, look};
use crate::levels::{CurrentLevel, LevelLoad};
use crate::map::{MapCollision, SKY};
use crate::retro::RetroScreen;
use crate::tuning::{Tuning, TuningAsset};

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        // Bevy note: `FixedUpdate` runs a fixed number of times per second (see
        // TICK_HZ in main.rs), however fast frames are drawn: movement behaves the
        // same at 30 or 240 fps. `Update` runs once per frame, for anything visual.
        // `.chain()` runs the systems in the listed order.
        app.init_resource::<PlayerStatus>()
            .init_resource::<ThirdPerson>()
            .init_resource::<Showcase>()
            .add_message::<RespawnPlayer>()
            .add_message::<RefillDash>()
            .add_message::<Knockback>()
            .add_systems(Startup, (spawn_player, spawn_body, spawn_hud))
            .add_systems(FixedUpdate, tick_movement.in_set(MovementTick))
            .add_systems(
                Update,
                (
                    respawn.after(LevelLoad),
                    aim,
                    place_camera,
                    place_body,
                    update_hud,
                )
                    .chain(),
            );
    }
}

/// Vertical field of view, degrees. Placeholder until we check the original's.
const FOV_DEGREES: f32 = 60.0;
/// Fall this far and you're put back at the spawn.
const KILL_HEIGHT: f32 = -2000.0;
/// Where the player is before the first level loads.
const START: Vec3 = Vec3::new(0.0, 1.0, 0.0);
/// How fast the view turns by itself behind the title screen, radians per second.
const SHOWCASE_TURN: f32 = 0.12;
/// Inches to metres, for the HUD.
const METRES_PER_UNIT: f32 = 0.0254;
/// Camera lean away from the wall while wall-running, degrees.
const WALL_RUN_ROLL_DEGREES: f32 = 8.0;
/// Extra field of view at the speed cap, degrees. Helps sell the speed. Grows
/// quickly at first (about half of it by wall-run speed), then more slowly.
const SPEED_FOV_DEGREES: f32 = 14.0;
/// Extra field of view at the moment of a dash, degrees, and how fast it fades
/// (per second).
const DASH_FOV_DEGREES: f32 = 12.0;
const DASH_KICK_FADE: f32 = 7.0;
/// How far the camera drops while sliding, units.
const SLIDE_EYE_DROP: f32 = 28.0;
/// How fast the camera eases towards step offsets, lean and FOV, per second.
/// Higher is snappier.
const VIEW_EASE_RATE: f32 = 12.0;
/// Third-person camera, from the eyes: this far back, up and right (over the
/// right shoulder, so the body doesn't block the crosshair), units.
const THIRD_PERSON_BACK: f32 = 130.0;
const THIRD_PERSON_UP: f32 = 16.0;
const THIRD_PERSON_RIGHT: f32 = 30.0;
/// Distance fog: clear up to here, units...
const FOG_START: f32 = 500.0;
/// ...and fully the sky colour from here.
const FOG_END: f32 = 4500.0;

/// The player's eye: the entity with the world camera, the input bindings and the
/// movement state. Other plugins (the weapon) attach to it and read where it looks.
#[derive(Component)]
pub struct PlayerCamera;

/// The fixed-tick step that moves the player. Order other per-tick systems
/// against it (e.g. the weapon fires from where the player ended up).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MovementTick;

/// Send this to put the player back at the level's start, facing its way.
#[derive(Message, Clone, Copy)]
pub struct RespawnPlayer;

/// Give the player back a dash charge (on a kill, say).
#[derive(Message, Clone, Copy)]
pub struct RefillDash;

/// Shove the player: added straight to their velocity (a rocket blast, say).
#[derive(Message, Clone, Copy)]
pub struct Knockback(pub Vec3);

/// Viewing from behind (true) or through the eyes (false). V toggles it.
#[derive(Resource, Default)]
pub struct ThirdPerson(pub bool);

/// The view slowly turns by itself (behind the title screen).
#[derive(Resource, Default)]
pub struct Showcase(pub bool);

/// The player's zombie body, seen in third person.
#[derive(Component)]
struct PlayerBody;

/// What the rest of the game may want to know about the player's movement,
/// updated every tick. Read-only for everyone but this module.
#[derive(Resource, Default)]
pub struct PlayerStatus {
    /// Bottom of the capsule (the feet).
    pub position: Vec3,
    pub velocity: Vec3,
    /// Which way the player looks, radians: 0 is -Z, positive turns left.
    pub yaw: f32,
    pub on_ground: bool,
    /// The grappling hook, while it's out.
    pub grapple: Option<Grapple>,
    /// Dash charges available, and the most there can be.
    pub stamina: f32,
    pub max_stamina: f32,
    pub dashing: bool,
    pub slamming: bool,
}

#[derive(Component)]
struct Player {
    state: MovementState,
    /// Position at the previous tick. The camera blends between the two so motion
    /// looks smooth even when the screen refreshes faster than the tick rate.
    previous: Vec3,
    /// Radians. Yaw 0 faces -Z; positive turns left. Pitch positive looks up.
    yaw: f32,
    pitch: f32,
    view: ViewSmoothing,
}

/// Camera-only easing. Purely visual: never fed back into the simulation.
#[derive(Default)]
struct ViewSmoothing {
    /// Eye height offset that hides the snap of walking up or down a step, units.
    /// Eases back to 0.
    step_offset: f32,
    /// Camera roll, radians. Leans away from the wall during a wall-run.
    roll: f32,
    /// Extra field of view at speed, degrees.
    fov_bonus: f32,
    /// How far the camera is currently lowered for a slide, units.
    slide_drop: f32,
    /// Dash kick, 1 the moment a dash starts, fading to 0: widens the view.
    dash_kick: f32,
}

fn spawn_player(mut commands: Commands, screen: Res<RetroScreen>) {
    commands.spawn((
        PlayerCamera,
        Player {
            state: MovementState::new(START),
            previous: START,
            yaw: 0.0,
            pitch: 0.0,
            view: ViewSmoothing::default(),
        },
        // leafwing adds the matching `ActionState<Action>` for us.
        input::default_bindings(),
        Camera3d::default(),
        // Draw into the low-resolution retro image, not the window. No
        // anti-aliasing: hard pixel edges are the look.
        screen.target(),
        Msaa::Off,
        DistanceFog {
            color: SKY,
            falloff: FogFalloff::Linear {
                start: FOG_START,
                end: FOG_END,
            },
            ..default()
        },
        // Default near/far planes assume metres; our units are inches.
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_DEGREES.to_radians(),
            near: 1.0,
            far: 20_000.0,
            ..default()
        }),
        Transform::from_translation(START),
    ));
}

fn spawn_body(
    mut commands: Commands,
    kit: Res<CharacterKit>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let body = character::spawn_zombie(
        &mut commands,
        &kit,
        &mut materials,
        Style::PLAYER,
        Transform::from_translation(START),
    );
    // Hidden in first person: the camera is inside it.
    commands
        .entity(body)
        .insert((PlayerBody, Visibility::Hidden));
}

/// Back to the level's start, facing its way, standing still. Works while the
/// game is paused, so the view behind the menus is right.
fn respawn(
    mut respawns: MessageReader<RespawnPlayer>,
    current: Res<CurrentLevel>,
    mut status: ResMut<PlayerStatus>,
    mut player: Single<&mut Player>,
) {
    if respawns.read().count() == 0 {
        return;
    }
    put_at_start(&mut player, &current);
    status.position = player.state.position;
    status.velocity = Vec3::ZERO;
    status.yaw = player.yaw;
}

fn put_at_start(player: &mut Player, current: &CurrentLevel) {
    player.state = MovementState::new(current.spawn);
    player.previous = current.spawn;
    player.yaw = current.facing;
    player.pitch = 0.0;
    player.view = ViewSmoothing::default();
}

/// Mouse and right stick turn the view every frame, for responsive aim. V
/// switches between first and third person.
fn aim(
    time: Res<Time>,
    real: Res<Time<Real>>,
    showcase: Res<Showcase>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    mut third_person: ResMut<ThirdPerson>,
    player: Single<(&mut Player, &ActionState<Action>)>,
) {
    let (mut player, actions) = player.into_inner();
    if showcase.0 {
        // Real time: the game is paused behind the title screen.
        player.yaw = (player.yaw + SHOWCASE_TURN * real.delta_secs()).rem_euclid(TAU);
        player.pitch = 0.05;
        return;
    }
    if actions.just_pressed(&Action::ToggleView) {
        third_person.0 = !third_person.0;
    }
    let mut turn = Vec2::ZERO; // x = yaw, y = pitch
    if input::cursor_captured(&cursor) {
        // Raw mouse deltas: +x is right, +y is down.
        let mouse = actions.axis_pair(&Action::Look);
        turn -= mouse * look::MOUSE_SENSITIVITY;
    }
    let stick = shape_stick(actions.axis_pair(&Action::LookStick), look::STICK_CURVE);
    turn += Vec2::new(
        -stick.x * look::STICK_YAW_SPEED,
        stick.y * look::STICK_PITCH_SPEED,
    ) * time.delta_secs();

    player.yaw = (player.yaw + turn.x).rem_euclid(TAU);
    player.pitch = (player.pitch + turn.y).clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
}

/// One simulation tick.
#[allow(clippy::too_many_arguments)]
fn tick_movement(
    time: Res<Time>,
    tuning: Res<Tuning>,
    tunings: Res<Assets<TuningAsset>>,
    map: Res<MapCollision>,
    current: Res<CurrentLevel>,
    mut status: ResMut<PlayerStatus>,
    mut refills: MessageReader<RefillDash>,
    mut knockbacks: MessageReader<Knockback>,
    boosts: Res<Boosts>,
    player: Single<(&mut Player, &ActionState<Action>)>,
) {
    // `let ... else` returns early if movement.ron hasn't finished loading.
    let Some(tuning) = tunings.get(&tuning.0) else {
        return;
    };
    // The run's heart rate makes you faster (see `heart.rs`).
    let tuning = &boosts.movement(tuning);
    let (mut player, actions) = player.into_inner();

    let wish = shape_stick(actions.clamped_axis_pair(&Action::Move), 1.0);
    let input = MoveInput {
        wish,
        yaw: player.yaw,
        pitch: player.pitch,
        // leafwing keeps a separate action state for FixedUpdate, so a press is
        // seen by exactly one tick even when a frame runs zero or several ticks.
        jump: actions.just_pressed(&Action::Jump),
        dash: actions.just_pressed(&Action::Dash),
        slam: actions.just_pressed(&Action::Slide),
        grapple: actions.pressed(&Action::Grapple),
        // No sprint: walking is already full speed.
        sprint: false,
        hang: actions.pressed(&Action::WallHang),
        slide: actions.pressed(&Action::Slide),
    };

    // `time` is the fixed clock here, so this is exactly one tick.
    let dt = time.delta_secs();
    // Outside forces first: blasts shove you, kills hand back dash charges.
    for Knockback(push) in knockbacks.read() {
        player.state.velocity += *push;
    }
    for _ in refills.read() {
        let stamina = player.state.stamina.min(tuning.dash_charges);
        player.state.stamina = (stamina + 1.0).min(tuning.dash_charges);
    }
    let next = step(&player.state, &input, tuning, &map.0, dt);
    let was_on_ground = player.state.on_ground;
    player.previous = player.state.position;
    player.state = next;

    // Walking up or down a step moves the feet a whole step in one tick, more
    // than the velocity explains. Move the camera's starting point with it (so
    // there's no jump between ticks) and ease the difference out over a few frames.
    if was_on_ground && next.on_ground {
        let snap = next.position.y - player.previous.y - next.velocity.y * dt;
        if snap.abs() > 1.0 {
            let limit = 2.0 * tuning.step_height;
            player.previous.y += snap;
            player.view.step_offset = (player.view.step_offset - snap).clamp(-limit, limit);
        }
    }

    if actions.just_pressed(&Action::Reset) || player.state.position.y < KILL_HEIGHT {
        put_at_start(&mut player, &current);
    }
    status.position = player.state.position;
    status.velocity = player.state.velocity;
    status.yaw = player.yaw;
    status.on_ground = player.state.on_ground;
    status.grapple = player.state.grapple;
    status.stamina = player.state.stamina;
    status.max_stamina = tuning.dash_charges;
    status.dashing = player.state.dash.is_some();
    status.slamming = player.state.slam.is_some();
}

#[allow(clippy::too_many_arguments)]
fn place_camera(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    tuning: Res<Tuning>,
    tunings: Res<Assets<TuningAsset>>,
    third_person: Res<ThirdPerson>,
    map: Res<MapCollision>,
    player: Single<(&mut Player, &mut Transform, &mut Projection)>,
) {
    let Some(tuning) = tunings.get(&tuning.0) else {
        return;
    };
    let (mut player, mut transform, mut projection) = player.into_inner();
    // Rust note: destructuring borrows each field separately, so we can change
    // `view` while reading the others.
    let Player {
        state,
        previous,
        yaw,
        pitch,
        view,
        ..
    } = &mut *player;

    // Closes the same fraction of the gap per second at any frame rate.
    let ease = 1.0 - (-VIEW_EASE_RATE * time.delta_secs()).exp();
    view.step_offset -= view.step_offset * ease;
    let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    let roll = match state.wall {
        // Wall on the right (normal pointing left): positive roll leans left.
        Some(wall) if wall.kind == WallKind::Run => {
            -wall.plane.normal.dot(right) * WALL_RUN_ROLL_DEGREES.to_radians()
        }
        _ => 0.0,
    };
    view.roll += (roll - view.roll) * ease;
    let speed = Vec2::new(state.velocity.x, state.velocity.z).length();
    let fast = ((speed - tuning.walk_speed) / (tuning.max_speed - tuning.walk_speed))
        .clamp(0.0, 1.0)
        .sqrt();
    view.fov_bonus += (fast * SPEED_FOV_DEGREES - view.fov_bonus) * ease;
    // A dash punches the view wide at once, then lets it settle.
    if state.dash.is_some() {
        view.dash_kick = 1.0;
    } else {
        view.dash_kick *= (-DASH_KICK_FADE * time.delta_secs()).exp();
    }
    let drop = if state.sliding { SLIDE_EYE_DROP } else { 0.0 };
    view.slide_drop += (drop - view.slide_drop) * ease;

    // How far we are between the last tick and the next one, 0..1.
    let blend = fixed.overstep_fraction();
    let feet = previous.lerp(state.position, blend);
    let eye = tuning.eye_height + view.step_offset - view.slide_drop;
    transform.translation = feet + Vec3::Y * eye;
    transform.rotation = Quat::from_euler(EulerRot::YXZ, *yaw, *pitch, view.roll);
    if third_person.0 {
        // Pull back behind the head, but not through walls: stop short of
        // whatever is in the way.
        let back =
            transform.rotation * Vec3::new(THIRD_PERSON_RIGHT, THIRD_PERSON_UP, THIRD_PERSON_BACK);
        let room = map
            .0
            .cast_ray(transform.translation, back)
            .map_or(1.0, |hit| (hit.fraction - 8.0 / back.length()).max(0.0));
        transform.translation += back * room;
    }
    if let Projection::Perspective(perspective) = projection.as_mut() {
        perspective.fov =
            (FOV_DEGREES + view.fov_bonus + view.dash_kick * DASH_FOV_DEGREES).to_radians();
    }
}

/// Stand the player's body where the player is (between ticks, like the camera),
/// facing where they look, and show it only in third person.
fn place_body(
    fixed: Res<Time<Fixed>>,
    third_person: Res<ThirdPerson>,
    player: Single<&Player>,
    body: Single<(&mut Transform, &mut Visibility, &mut Gait), With<PlayerBody>>,
) {
    let (mut transform, mut visibility, mut gait) = body.into_inner();
    let state = &player.state;
    transform.translation = player
        .previous
        .lerp(state.position, fixed.overstep_fraction());
    transform.rotation = Quat::from_rotation_y(player.yaw);
    gait.speed = Vec2::new(state.velocity.x, state.velocity.z).length();
    gait.grounded = state.on_ground;
    *visibility = if third_person.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
}

/// Deadzone, then a response curve. Keeps the direction, reshapes the length.
fn shape_stick(v: Vec2, curve: f32) -> Vec2 {
    let len = v.length();
    if len <= look::STICK_DEADZONE {
        return Vec2::ZERO;
    }
    let scaled = ((len - look::STICK_DEADZONE) / (1.0 - look::STICK_DEADZONE)).min(1.0);
    v / len * scaled.powf(curve)
}

#[derive(Component)]
struct Hud;

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(12.0),
            ..default()
        },
    ));
}

/// Every control and a few tips, for the menus.
pub const CONTROLS: &str = "WASD move   mouse look   left mouse fire   1-5 / wheel weapons   F machete   R reload\n\
                            Space jump   Shift dash   Ctrl/C slide (air: slam)   E grapple   right mouse wall-hang\n\
                            Q heart (fire squeezes it)   V third person   Backspace back to the start   Esc menu\n\
                            Pad: RT fire  D-pad down heart  Y weapon  R3 machete  X reload  A jump  RB dash\n\
                            B slide/slam  LB grapple  LT wall-hang  D-pad up third person  Start menu\n\
                            \n\
                            Dash: untouchable while it lasts, 3 charges, every kill gives one back.\n\
                            Jump during a ground dash for a dash jump. Slide-hop to build speed.\n\
                            Slam, then jump as you land to bounce high. Wall jump as often as you like.\n\
                            Rocket at your feet + jump = rocket jump.";

fn update_hud(
    player: Single<&Player>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    mut text: Single<&mut Text, With<Hud>>,
) {
    let state = &player.state;
    let v = state.velocity;
    let speed = Vec2::new(v.x, v.z).length();
    let mode = match (state.on_ground, state.wall) {
        _ if state.grapple.is_some_and(|g| g.attached) => "grappling".to_string(),
        _ if state.dash.is_some() => "dashing".to_string(),
        _ if state.slam.is_some() => "SLAM".to_string(),
        (true, _) if state.sliding => "sliding".to_string(),
        (true, _) => "on ground".to_string(),
        (false, Some(wall)) if wall.kind == WallKind::Run => {
            format!("wall-run {:.2}s", wall.time)
        }
        (false, Some(wall)) => format!("wall-hang {:.2}s", wall.time),
        (false, None) => "in air".to_string(),
    };
    let hint = if input::cursor_captured(&cursor) {
        "Esc: menu and controls"
    } else {
        "Click to capture the mouse"
    };
    // Unlimited wall jumps are stored as u32::MAX.
    let wall_jumps = if state.wall_jumps > 1_000_000 {
        "unlimited".to_string()
    } else {
        state.wall_jumps.to_string()
    };
    text.0 = format!(
        "speed {speed:4.0} u/s ({:4.1} m/s)   vertical {:+5.0} u/s   {mode}   wall jumps {wall_jumps}\n\n{hint}",
        speed * METRES_PER_UNIT,
        v.y,
    );
}
