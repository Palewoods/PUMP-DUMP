//! Every movement number lives here and is loaded from a `.ron` file, so tuning
//! never needs a code change. There's deliberately no `Default`: values come from
//! data (`crates/pumpdump/assets/movement.ron`).

use serde::Deserialize;

use crate::collision::Capsule;

/// Units: game units (~1 inch), seconds, degrees.
#[derive(Debug, Clone, PartialEq, Deserialize)]
// Rust note: `deny_unknown_fields` turns a typo in the .ron file into a load error
// instead of a silently ignored value.
#[serde(deny_unknown_fields)]
pub struct MovementTuning {
    pub capsule_radius: f32,
    /// Feet to top of head.
    pub capsule_height: f32,
    /// Feet to camera.
    pub eye_height: f32,

    /// Top ground speed without sprint, units/s.
    pub walk_speed: f32,
    /// Top ground speed while sprinting forwards, units/s.
    pub sprint_speed: f32,
    /// How fast ground speed approaches the wished-for speed, units/s².
    pub ground_accel: f32,
    /// How fast ground speed bleeds off with no input, units/s².
    pub ground_friction: f32,
    /// How fast ground speed above the wished-for speed (carried in from a jump or
    /// wall-run) bleeds off while you keep moving, units/s². Lower than
    /// `ground_accel`, so landing doesn't throw your speed away.
    pub overspeed_decel: f32,
    /// Steering acceleration in the air, units/s². Never raises speed above the
    /// faster of your current speed and the wished-for speed.
    pub air_accel: f32,
    /// Hard cap on flat speed, however it was gained, units/s.
    pub max_speed: f32,

    /// Flat speed needed to start a slide, units/s.
    pub slide_start_speed: f32,
    /// A slide ends when it slows below this, units/s.
    pub slide_min_speed: f32,
    /// Speed added when a slide starts, units/s (still capped by `max_speed`).
    pub slide_boost: f32,
    /// Seconds after a boost before starting another slide boosts again.
    pub slide_boost_cooldown: f32,
    /// How fast a slide on flat ground slows down, units/s².
    pub slide_friction: f32,
    /// How fast the stick can turn a slide, units/s². Never adds speed.
    pub slide_steer: f32,
    /// A slide always goes at least this fast, even started from a standstill,
    /// units/s.
    pub slide_speed: f32,

    /// Dash charges when full.
    pub dash_charges: f32,
    /// Dash charges regained per second (not while dashing).
    pub dash_recharge: f32,
    /// Speed of a dash, units/s. Gravity is off while dashing.
    pub dash_speed: f32,
    /// How long a dash lasts, seconds.
    pub dash_time: f32,
    /// Flat speed you come out of a dash with, if you weren't already faster,
    /// units/s.
    pub dash_exit_speed: f32,
    /// Flat speed of a dash jump (jumping out of a ground dash), units/s.
    pub dash_jump_speed: f32,

    /// Downward speed of a ground slam (slide pressed in the air), units/s.
    pub slam_speed: f32,
    /// Seconds after landing a slam in which a jump bounces higher.
    pub slam_bounce_time: f32,
    /// A slam bounce reaches this fraction of the height the slam fell...
    pub slam_bounce_ratio: f32,
    /// ...but no more than this, units.
    pub slam_bounce_max_height: f32,

    /// How far the grappling hook reaches, units.
    pub grapple_range: f32,
    /// How fast the hook flies out, units/s.
    pub grapple_hook_speed: f32,
    /// Speed it reels you in at, units/s.
    pub grapple_pull_speed: f32,
    /// How fast your velocity swings round towards the hook, units/s².
    pub grapple_pull_accel: f32,
    /// The grapple lets go when your eyes get this close to the hook, units.
    pub grapple_release_distance: f32,

    /// Downward acceleration, units/s².
    pub gravity: f32,
    /// Apex height of a standing jump, units. Launch speed is derived from it.
    pub jump_height: f32,
    /// Seconds after running off a ledge in which jump still works.
    pub coyote_time: f32,
    /// A jump pressed up to this many seconds before landing (or reaching a wall)
    /// still happens when you get there.
    pub jump_buffer_time: f32,
    /// Extra jumps allowed in the air (1 = double jump). Refilled on landing and
    /// when you grab a wall.
    pub air_jumps: u32,
    /// Height an air jump adds from a standstill, units. Launch speed is derived
    /// from it, and replaces any fall speed.
    pub air_jump_height: f32,
    /// How far an air jump turns your flat motion towards the stick: 0 not at all,
    /// 1 fully. Speed is kept (or raised to the wished-for speed).
    pub air_jump_redirect: f32,

    /// Top speed along a wall while holding towards your run direction, units/s.
    pub wall_run_speed: f32,
    /// How fast wall-run speed approaches `wall_run_speed`, or bleeds off when you
    /// stop pushing along the wall, units/s².
    pub wall_run_accel: f32,
    /// Slower than this along the wall and a run can't start, or ends, units/s.
    pub wall_run_min_speed: f32,
    /// Downward acceleration while wall-running, units/s².
    pub wall_run_gravity: f32,
    /// Fastest a wall-run sinks, units/s. Small, so long runs keep their height.
    pub wall_run_fall_speed: f32,
    /// When a run starts, upward speed is capped to this and a fall is caught
    /// (vertical speed raised to 0), units/s.
    pub wall_run_rise_speed: f32,
    /// Longest wall-run, seconds. Then you drop off.
    pub wall_run_max_time: f32,
    /// Longest wall-hang, seconds. Then you drop off.
    pub wall_hang_max_time: f32,
    /// Feet must be at least this high above walkable ground to grab a wall, units.
    pub wall_min_height: f32,
    /// Speed away from the wall added by a wall-jump, units/s. The upward part is
    /// the normal jump speed.
    pub wall_jump_push: f32,
    /// Wall jumps (kick-offs and hops) allowed before you land again. `None` for
    /// unlimited (in the .ron file: `wall_jumps: None` or `wall_jumps: Some(3)`).
    pub wall_jumps: Option<u32>,
    /// Height a wall hop gains, units. A hop is a jump with the stick along the
    /// wall rather than away from it: you stay on the same wall and its run timer
    /// starts over, so hops chain into one long run.
    pub wall_hop_height: f32,

    /// Steepest slope that counts as ground, degrees from flat.
    pub max_slope_deg: f32,
    /// Tallest ledge walked up without jumping, units.
    pub step_height: f32,
}

impl MovementTuning {
    pub fn capsule(&self) -> Capsule {
        Capsule {
            radius: self.capsule_radius,
            height: self.capsule_height.max(2.0 * self.capsule_radius),
        }
    }

    /// Initial upward speed that reaches `jump_height` under `gravity` (v² = 2gh).
    pub fn jump_speed(&self) -> f32 {
        (2.0 * self.gravity * self.jump_height).max(0.0).sqrt()
    }

    /// Initial upward speed of a wall hop. It rises against wall-run gravity, so
    /// that's what reaches `wall_hop_height`.
    pub fn wall_hop_speed(&self) -> f32 {
        (2.0 * self.wall_run_gravity * self.wall_hop_height)
            .max(0.0)
            .sqrt()
    }

    /// Wall jumps you get back on landing. Unlimited counts as `u32::MAX`, which
    /// never runs down.
    pub fn wall_jumps_per_airtime(&self) -> u32 {
        self.wall_jumps.unwrap_or(u32::MAX)
    }

    /// Initial upward speed of an air jump, from `air_jump_height`.
    pub fn air_jump_speed(&self) -> f32 {
        (2.0 * self.gravity * self.air_jump_height).max(0.0).sqrt()
    }

    /// A surface is walkable if its normal's up component is at least this.
    pub fn min_ground_normal_y(&self) -> f32 {
        self.max_slope_deg.to_radians().cos()
    }
}
