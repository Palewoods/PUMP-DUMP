//! The player: feeds actions into `pumpdump_movement::step` every fixed tick, and places
//! the first-person camera every rendered frame.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
use leafwing_input_manager::prelude::*;
use pumpdump_movement::{Grapple, MoveInput, MovementState, WallKind, step};

use crate::input::{self, Action, look};
use crate::map::{MapCollision, SKY, SPAWN};
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
            .add_systems(Startup, (spawn_player, spawn_hud))
            .add_systems(FixedUpdate, tick_movement.in_set(MovementTick))
            .add_systems(Update, (aim, place_camera, update_hud).chain());
    }
}

/// Vertical field of view, degrees. Placeholder until we check the original's.
const FOV_DEGREES: f32 = 60.0;
/// Fall this far and you're put back at the spawn.
const KILL_HEIGHT: f32 = -2000.0;
/// Inches to metres, for the HUD.
const METRES_PER_UNIT: f32 = 0.0254;
/// Camera lean away from the wall while wall-running, degrees.
const WALL_RUN_ROLL_DEGREES: f32 = 8.0;
/// Extra field of view at the speed cap, degrees. Helps sell the speed. Grows
/// quickly at first (about half of it by wall-run speed), then more slowly.
const SPEED_FOV_DEGREES: f32 = 14.0;
/// How far the camera drops while sliding, units.
const SLIDE_EYE_DROP: f32 = 28.0;
/// How fast the camera eases towards step offsets, lean and FOV, per second.
/// Higher is snappier.
const VIEW_EASE_RATE: f32 = 12.0;
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

/// What the rest of the game may want to know about the player's movement,
/// updated every tick. Read-only for everyone but this module.
#[derive(Resource, Default)]
pub struct PlayerStatus {
    pub velocity: Vec3,
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
}

fn spawn_player(mut commands: Commands, screen: Res<RetroScreen>) {
    commands.spawn((
        PlayerCamera,
        Player {
            state: MovementState::new(SPAWN),
            previous: SPAWN,
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
        Transform::from_translation(SPAWN),
    ));
}

/// Mouse and right stick turn the view every frame, for responsive aim.
fn aim(
    time: Res<Time>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    player: Single<(&mut Player, &ActionState<Action>)>,
) {
    let (mut player, actions) = player.into_inner();
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
fn tick_movement(
    time: Res<Time>,
    tuning: Res<Tuning>,
    tunings: Res<Assets<TuningAsset>>,
    map: Res<MapCollision>,
    mut status: ResMut<PlayerStatus>,
    player: Single<(&mut Player, &ActionState<Action>)>,
) {
    // `let ... else` returns early if movement.ron hasn't finished loading.
    let Some(tuning) = tunings.get(&tuning.0) else {
        return;
    };
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
        player.state = MovementState::new(SPAWN);
        player.previous = SPAWN;
        player.view = ViewSmoothing::default();
    }
    status.velocity = player.state.velocity;
    status.on_ground = player.state.on_ground;
    status.grapple = player.state.grapple;
    status.stamina = player.state.stamina;
    status.max_stamina = tuning.dash_charges;
    status.dashing = player.state.dash.is_some();
    status.slamming = player.state.slam.is_some();
}

fn place_camera(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    tuning: Res<Tuning>,
    tunings: Res<Assets<TuningAsset>>,
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
    let drop = if state.sliding { SLIDE_EYE_DROP } else { 0.0 };
    view.slide_drop += (drop - view.slide_drop) * ease;

    // How far we are between the last tick and the next one, 0..1.
    let blend = fixed.overstep_fraction();
    let feet = previous.lerp(state.position, blend);
    let eye = tuning.eye_height + view.step_offset - view.slide_drop;
    transform.translation = feet + Vec3::Y * eye;
    transform.rotation = Quat::from_euler(EulerRot::YXZ, *yaw, *pitch, view.roll);
    if let Projection::Perspective(perspective) = projection.as_mut() {
        perspective.fov = (FOV_DEGREES + view.fov_bonus).to_radians();
    }
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

const CONTROLS: &str = "WASD move   mouse look   left mouse fire   R reload   Space jump   Shift dash   Ctrl/C slide (in the air: slam)\n\
                        E grapple (hold)   right mouse wall-hang   Backspace back to spawn   Esc free mouse\n\
                        Controller: sticks   RT fire   X reload   A jump   RB dash   B slide/slam   LB grapple   LT wall-hang   Back to spawn\n\
                        Dash costs a charge (3, they refill). Jump during a ground dash for a long dash jump.\n\
                        Slide any time on the ground; slide-hop to build speed. Slam, then jump as you land to bounce high.\n\
                        Touch any wall in the air and jump to wall jump, as often as you like. Run along walls for up to 12 s.\n\
                        Edit assets/movement.ron while playing; it reloads on save.";

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
        CONTROLS
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
