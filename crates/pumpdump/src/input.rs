//! Named input actions. Gameplay code only asks "is Jump pressed?", never "is
//! Space pressed?", so keyboard/mouse and controller are equal from the start and
//! rebinding later only touches `default_bindings`.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use leafwing_input_manager::prelude::*;

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InputManagerPlugin::<Action>::default())
            .add_systems(Update, capture_cursor);
    }
}

// Rust note: `derive` generates trait implementations for us. `Actionlike` (from
// leafwing) is what lets this enum be used as a set of bindable actions.
#[derive(Actionlike, PartialEq, Eq, Hash, Clone, Copy, Debug, Reflect)]
pub enum Action {
    /// Left stick or WASD. x = right, y = forwards.
    #[actionlike(DualAxis)]
    Move,
    /// Mouse movement, in pixels this frame.
    #[actionlike(DualAxis)]
    Look,
    /// Right stick, -1..1. Kept apart from `Look` because it's a rate, not a distance.
    #[actionlike(DualAxis)]
    LookStick,
    Jump,
    /// A quick burst of speed, paid for with a dash charge.
    Dash,
    /// Held: throw the grappling hook and get pulled to where it sticks.
    Grapple,
    /// Held: grab the wall you're touching in the air.
    WallHang,
    /// Held: slide on the ground. Pressed in the air: ground slam.
    Slide,
    /// Fire the weapon. One shot per press.
    Fire,
    /// Reload the weapon.
    Reload,
    /// Back to the spawn point.
    Reset,
}

pub fn default_bindings() -> InputMap<Action> {
    InputMap::default()
        .with_dual_axis(Action::Move, VirtualDPad::wasd())
        .with_dual_axis(Action::Move, GamepadStick::LEFT)
        .with_dual_axis(Action::Look, MouseMove::default())
        .with_dual_axis(Action::LookStick, GamepadStick::RIGHT)
        .with(Action::Jump, KeyCode::Space)
        .with(Action::Jump, GamepadButton::South)
        .with(Action::Dash, KeyCode::ShiftLeft)
        .with(Action::Dash, GamepadButton::RightTrigger)
        .with(Action::Grapple, KeyCode::KeyE)
        .with(Action::Grapple, MouseButton::Back)
        .with(Action::Grapple, GamepadButton::LeftTrigger)
        .with(Action::WallHang, MouseButton::Right)
        .with(Action::WallHang, GamepadButton::LeftTrigger2)
        .with(Action::Slide, KeyCode::ControlLeft)
        .with(Action::Slide, KeyCode::KeyC)
        .with(Action::Slide, GamepadButton::East)
        .with(Action::Fire, MouseButton::Left)
        .with(Action::Fire, GamepadButton::RightTrigger2)
        .with(Action::Reload, KeyCode::KeyR)
        .with(Action::Reload, GamepadButton::West)
        .with(Action::Reset, KeyCode::Backspace)
        .with(Action::Reset, GamepadButton::Select)
}

/// Look feel. Not movement tuning (it doesn't change the simulation), so it lives
/// here rather than in movement.ron. Will become player settings.
pub mod look {
    /// Radians of turn per pixel of mouse movement.
    pub const MOUSE_SENSITIVITY: f32 = 0.0022;
    /// Radians per second at full right-stick deflection.
    pub const STICK_YAW_SPEED: f32 = 3.2;
    pub const STICK_PITCH_SPEED: f32 = 2.2;
    /// Stick deflection is raised to this power: small tilts aim finely, full tilt
    /// still turns at full speed.
    pub const STICK_CURVE: f32 = 2.0;
    /// Below this deflection the stick counts as centred (worn sticks drift).
    pub const STICK_DEADZONE: f32 = 0.12;
}

/// Click in the window to capture the mouse for look; Escape releases it.
fn capture_cursor(
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if mouse.just_pressed(MouseButton::Left) {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
    if keys.just_pressed(KeyCode::Escape) {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

pub fn cursor_captured(cursor: &CursorOptions) -> bool {
    cursor.grab_mode != CursorGrabMode::None
}
