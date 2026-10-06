//! Pilot movement as a pure fixed-tick function: `(state, input, world) -> state`.
//!
//! No Bevy, no `std::fs`, so it runs in plain unit tests, in the sandbox, and
//! later on a server or in a browser. Same inputs give the same result, which is
//! what replays and network prediction need.
//!
//! Units are game units (~1 inch) and seconds. Y is up. Yaw 0 faces -Z and
//! positive yaw turns left, like a Bevy camera.
//!
//! The collision approach is the general "collide and slide" technique: sweep the
//! capsule, stop just short of what it hits, remove the velocity going into that
//! surface, and sweep again with what's left of the tick.
//!
//! Each tick the pilot is doing one thing, checked in this order: being pulled by
//! the grapple, dashing, ground-slamming, on the ground, on a wall (running or
//! hanging), or in the air. Walls are only looked for in the air.

pub mod collision;
mod tuning;

use std::f32::consts::FRAC_PI_4;

use glam::{Vec2, Vec3};

pub use collision::{Capsule, CollisionWorld, Hit, StaticWorld};
pub use tuning::MovementTuning;

/// Gap kept between the capsule and every surface, so the next sweep doesn't start
/// already touching. Units.
const SKIN: f32 = 0.1;
/// How far below the feet we look for ground. Must be bigger than `SKIN`.
const GROUND_PROBE: f32 = 2.0;
/// Most surfaces one sweep can slide off before giving up for this tick.
const MAX_BUMPS: usize = 4;
/// Moving away from the ground faster than this (units/s) means we've left it.
const LIFTOFF_SPEED: f32 = 1.0;
/// An edge contact counts as possible ground only if its normal is at least this
/// upward. Below that, the edge is beside the capsule rather than under it.
const EDGE_MIN_NORMAL_Y: f32 = 0.1;
/// How far beside the capsule we look for a wall to run on or hang from. Units.
const WALL_PROBE: f32 = 6.0;
/// A surface is a wall if its normal's up component is within ±this (about 20°
/// either side of vertical).
const WALL_MAX_NORMAL_Y: f32 = 0.35;
/// Pointing the stick away from the wall more than this (cosine of the angle from
/// the wall's normal) lets go of it.
const WALL_LEAVE_DOT: f32 = 0.6;
/// Jumping with the stick pointed away from the wall more than this (cosine, about
/// 15°) kicks off it; less, and it's a hop that stays on the wall.
const WALL_KICK_DOT: f32 = 0.25;
/// Pull into the wall while on it, units/s, so runs follow walls that bend away.
/// Only used for the move; never left in the velocity.
const WALL_STICK_SPEED: f32 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementState {
    /// Bottom of the capsule.
    pub position: Vec3,
    /// Units/s.
    pub velocity: Vec3,
    pub on_ground: bool,
    /// Wall-running or wall-hanging, if either.
    pub wall: Option<WallContact>,
    /// The wall we last let go of. It can't be grabbed again until we land, so you
    /// can't climb forever by jumping off and back onto the same wall.
    pub last_wall: Option<WallPlane>,
    /// Seconds left in which a jump still counts as from the ground (coyote time).
    pub coyote: f32,
    /// Seconds left in which an early jump press still fires (jump buffer).
    pub jump_buffer: f32,
    /// Air jumps (double jumps) left before we next land or grab a wall.
    pub air_jumps: u32,
    /// Wall jumps (kick-offs and hops) left before we next land.
    pub wall_jumps: u32,
    /// Sliding along the ground.
    pub sliding: bool,
    /// Seconds until starting a slide gives a speed boost again.
    pub slide_cooldown: f32,
    /// Dashing, if a dash is under way.
    pub dash: Option<Dash>,
    /// Dash charges available. Fractions build up between dashes; a dash needs a
    /// whole one.
    pub stamina: f32,
    /// Ground-slamming: the height the slam started from.
    pub slam: Option<f32>,
    /// Just landed a slam: a jump now bounces higher.
    pub slam_bounce: Option<SlamBounce>,
    /// The grappling hook, while it's out.
    pub grapple: Option<Grapple>,
    /// The grapple button has been let go since the last throw, so holding it
    /// down doesn't keep re-firing.
    pub grapple_armed: bool,
}

impl MovementState {
    /// Standing still at `position`. Ground is detected on the first tick.
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            velocity: Vec3::ZERO,
            on_ground: false,
            wall: None,
            last_wall: None,
            coyote: 0.0,
            jump_buffer: 0.0,
            // Both refilled as soon as we touch the ground.
            air_jumps: 0,
            wall_jumps: 0,
            sliding: false,
            slide_cooldown: 0.0,
            dash: None,
            // Full: trimmed to the tuning's number of charges on the first tick.
            stamina: f32::MAX,
            slam: None,
            slam_bounce: None,
            grapple: None,
            grapple_armed: true,
        }
    }
}

/// A short burst of fixed speed in one direction, ignoring gravity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dash {
    /// Flat unit vector.
    pub dir: Vec3,
    pub time_left: f32,
    /// Flat speed you leave the dash with.
    pub exit_speed: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlamBounce {
    pub time_left: f32,
    /// Upward speed a jump gets now (if more than a normal jump's).
    pub speed: f32,
}

/// The grappling hook: flying out towards `anchor`, then pulling once it's there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grapple {
    /// Where the hook is headed: the surface it will hit, or the end of its range.
    pub anchor: Vec3,
    /// It's going to hit something (rather than miss and reel back in).
    pub hooked: bool,
    /// How far the hook has flown so far, units.
    pub reach: f32,
    /// Stuck in: pulling.
    pub attached: bool,
}

impl Grapple {
    /// Where the hook is now, seen from `eye`.
    pub fn tip(&self, eye: Vec3) -> Vec3 {
        if self.attached {
            self.anchor
        } else {
            eye + (self.anchor - eye).normalize_or_zero() * self.reach
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallKind {
    Run,
    Hang,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WallContact {
    pub kind: WallKind,
    pub plane: WallPlane,
    /// Seconds since this run or hang started.
    pub time: f32,
}

/// The flat face of a wall: every point `p` on it has `normal.dot(p) == offset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WallPlane {
    /// Horizontal unit vector pointing out of the wall.
    pub normal: Vec3,
    pub offset: f32,
}

impl WallPlane {
    /// Same face, give or take a little: facing the same way and in the same place.
    fn same_as(&self, other: &WallPlane) -> bool {
        self.normal.dot(other.normal) > 0.95 && (self.offset - other.offset).abs() < 8.0
    }
}

/// One tick of player intent. The input layer (keyboard, controller, replay, network)
/// builds this; movement never sees raw buttons.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MoveInput {
    /// x = strafe right, y = forwards. Length up to 1 (a half-tilted stick gives less).
    pub wish: Vec2,
    /// Facing around the up axis, radians.
    pub yaw: f32,
    /// Looking up (positive) or down, radians. Aims the grapple.
    pub pitch: f32,
    /// The jump button went down since the last tick.
    pub jump: bool,
    /// The dash button went down since the last tick.
    pub dash: bool,
    /// The slide button went down since the last tick. In the air, that's a
    /// ground slam.
    pub slam: bool,
    /// The grapple button is held.
    pub grapple: bool,
    /// Sprint is held. Only counts while moving forwards.
    pub sprint: bool,
    /// Wall-hang is held. Grabs a wall you're touching in the air and holds still.
    pub hang: bool,
    /// Slide (crouch) is held. Slides when you're on the ground and moving fast.
    pub slide: bool,
}

/// Advance one fixed tick of `dt` seconds.
pub fn step(
    state: &MovementState,
    input: &MoveInput,
    tuning: &MovementTuning,
    world: &impl CollisionWorld,
    dt: f32,
) -> MovementState {
    // Garbage in (a zero tick, NaN from a broken controller) shouldn't corrupt the state.
    if dt.is_nan()
        || dt <= 0.0
        || !input.wish.is_finite()
        || !input.yaw.is_finite()
        || !input.pitch.is_finite()
    {
        return *state;
    }
    let body = Body {
        capsule: tuning.capsule(),
        min_ground_y: tuning.min_ground_normal_y(),
    };
    let mut pos = body.push_out(world, state.position);
    let mut vel = state.velocity;
    let wish = wish_velocity(input, tuning);

    // A jump press is remembered for a moment, so pressing it just before landing
    // or reaching a wall still jumps.
    let buffered = (state.jump_buffer - dt).max(0.0);
    let wants_jump = input.jump || buffered > 0.0;
    let mut jump_buffer = if input.jump {
        tuning.jump_buffer_time
    } else {
        buffered
    };

    // Dash charges refill between dashes.
    let mut stamina = state.stamina.min(tuning.dash_charges);
    if state.dash.is_none() {
        stamina = (stamina + tuning.dash_recharge * dt).min(tuning.dash_charges);
    }

    // Ground is re-checked every tick rather than trusted from the last one.
    let mut ground = body.find_ground(world, pos, vel);
    let mut coyote = if ground.is_some() {
        tuning.coyote_time
    } else {
        (state.coyote - dt).max(0.0)
    };

    // The grapple: throw, fly, stick, pull. Being pulled lifts you off the ground.
    let eye = pos + Vec3::Y * tuning.eye_height;
    let (mut grapple, grapple_armed) = update_grapple(state, input, tuning, world, eye, dt);
    let mut pulling = grapple.is_some_and(|g| g.attached);
    let hooked_now = pulling && !state.grapple.is_some_and(|g| g.attached);
    if pulling {
        ground = None;
    }

    // Ground slam: slide pressed in the air. Landing one opens a window in which
    // a jump bounces you up, higher the further you fell.
    let mut slam = state.slam;
    let mut slam_bounce = state.slam_bounce.and_then(|bounce| {
        let time_left = bounce.time_left - dt;
        (time_left > 0.0).then_some(SlamBounce {
            time_left,
            ..bounce
        })
    });
    if ground.is_some()
        && let Some(from) = slam.take()
    {
        slam_bounce = Some(slam_bounce_from(from - pos.y, tuning));
        vel = Vec3::ZERO;
    }
    if input.slam && ground.is_none() && !pulling && slam.is_none() {
        slam = Some(pos.y);
    }

    // Dash: a quick fixed-speed burst where the stick points (or straight ahead),
    // paid for with a charge.
    let mut dash = state.dash;
    if input.dash && dash.is_none() && stamina >= 1.0 && slam.is_none() && !pulling {
        stamina -= 1.0;
        dash = Some(Dash {
            dir: horizontal(wish)
                .try_normalize()
                .unwrap_or_else(|| facing(input.yaw)),
            time_left: tuning.dash_time,
            exit_speed: horizontal(vel).length().max(tuning.dash_exit_speed),
        });
    }
    if slam.is_some() || pulling {
        dash = None;
    }

    // Walls. Not while being pulled or slamming.
    let walls = if ground.is_some() {
        WallUpdate::default()
    } else if pulling || slam.is_some() {
        WallUpdate {
            last_wall: state.last_wall,
            ..WallUpdate::default()
        }
    } else {
        body.next_wall(world, state, input, tuning, pos, dt)
    };
    let (mut wall, mut last_wall, touching) = (walls.contact, walls.last_wall, walls.touching);
    // Starting a run catches a fall and trims a jump, so you run along the wall
    // rather than up it.
    if wall.map(|w| w.kind) == Some(WallKind::Run)
        && state.wall.map(|w| w.kind) != Some(WallKind::Run)
    {
        vel.y = vel.y.clamp(0.0, tuning.wall_run_rise_speed);
    }
    let mut air_jumps = if ground.is_some() || wall.is_some() || hooked_now {
        tuning.air_jumps
    } else {
        state.air_jumps
    };
    let mut wall_jumps = if ground.is_some() {
        tuning.wall_jumps_per_airtime()
    } else {
        state.wall_jumps
    };

    // Sliding: hold slide on the ground. A slide never drops below `slide_speed`,
    // and starting one boosts your speed (at most once per
    // `slide_boost_cooldown`), so landing in a slide and jumping out of it again
    // builds speed, up to `max_speed`. Sliding out of a dash keeps the dash's
    // speed. This runs before jumping so a slide-hop that lands and jumps on the
    // same tick still boosts.
    let mut slide_cooldown = (state.slide_cooldown - dt).max(0.0);
    let flat_speed = horizontal(vel).length();
    let slide_needs = if state.sliding {
        tuning.slide_min_speed
    } else {
        tuning.slide_start_speed
    };
    let mut sliding = ground.is_some() && input.slide && flat_speed >= slide_needs;
    if sliding && !state.sliding {
        let mut dir = horizontal(vel)
            .try_normalize()
            .or_else(|| horizontal(wish).try_normalize())
            .unwrap_or_else(|| facing(input.yaw));
        let mut speed = flat_speed.max(tuning.slide_speed);
        if let Some(dashing) = dash.take() {
            dir = dashing.dir;
            speed = speed.max(tuning.dash_speed);
        }
        // The boost goes on top, so every slide start feels like a kick.
        if slide_cooldown <= 0.0 {
            speed += tuning.slide_boost;
            slide_cooldown = tuning.slide_boost_cooldown;
        }
        let speed = speed.min(tuning.max_speed.max(flat_speed));
        let flat = dir * speed;
        vel = Vec3::new(flat.x, vel.y, flat.z);
    }

    // Jumps, in order of preference: off (or along) a wall you're on, off the
    // ground (or just after leaving it), off a wall you're touching, then a
    // double jump. Jumping lets go of the grapple.
    if wants_jump {
        let mut jumped = false;
        if pulling {
            grapple = None;
            pulling = false;
        }
        if let Some(contact) = wall {
            if wall_jumps > 0 {
                spend_wall_jump(&mut wall_jumps, tuning);
                jumped = true;
                if contact.kind == WallKind::Run && is_wall_hop(wish, contact.plane.normal) {
                    // Hop: stay on the wall, rise again, and restart the run timer.
                    vel.y = tuning.wall_hop_speed();
                    wall = Some(WallContact {
                        time: 0.0,
                        ..contact
                    });
                } else {
                    vel = wall_jump(vel, contact.plane.normal, tuning);
                    last_wall = Some(contact.plane);
                    wall = None;
                }
            } else {
                // Out of wall jumps: let go. The double jump below can still fire.
                last_wall = Some(contact.plane);
                wall = None;
            }
        }
        if !jumped && wall.is_none() {
            if ground.is_some() || coyote > 0.0 {
                vel.y = tuning.jump_speed();
                if let Some(bounce) = slam_bounce.take() {
                    vel.y = vel.y.max(bounce.speed);
                }
                // Dash jump: jumping out of a ground dash keeps most of its speed.
                if let Some(dashing) = dash.take() {
                    let flat = dashing.dir * tuning.dash_jump_speed.max(horizontal(vel).length());
                    vel = Vec3::new(flat.x, vel.y, flat.z);
                }
                ground = None;
                sliding = false;
                jumped = true;
            } else if let Some(plane) = touching.filter(|_| wall_jumps > 0) {
                spend_wall_jump(&mut wall_jumps, tuning);
                vel = wall_jump(vel, plane.normal, tuning);
                last_wall = Some(plane);
                dash = None;
                jumped = true;
            } else if air_jumps > 0 {
                vel = air_jump(vel, wish, tuning);
                air_jumps -= 1;
                dash = None;
                jumped = true;
            }
        }
        if jumped {
            jump_buffer = 0.0;
            coyote = 0.0;
        }
    }

    if pulling && let Some(hook) = grapple {
        // Reel in: velocity swings round towards the anchor at pull speed.
        let towards = (hook.anchor - (pos + Vec3::Y * tuning.eye_height)).normalize_or_zero();
        vel = move_towards(
            vel,
            towards * tuning.grapple_pull_speed,
            tuning.grapple_pull_accel * dt,
        );
        let pulled = body.slide(world, pos, vel, dt, false);
        (pos, vel) = (pulled.pos, pulled.vel);
    } else if let Some(mut dashing) = dash {
        let mut burst = dashing.dir * tuning.dash_speed;
        if let Some(normal) = ground {
            burst = along_ground(burst, normal);
        }
        let moved = body.slide(world, pos, burst, dt, ground.is_some());
        pos = moved.pos;
        if ground.is_some() {
            pos = body
                .snap_down(world, pos, tuning.step_height)
                .unwrap_or(pos);
        }
        // How much of the dash got through (less if it ran into something).
        let free = (horizontal(moved.vel).length() / tuning.dash_speed).min(1.0);
        dashing.time_left -= dt;
        if dashing.time_left > 0.0 {
            vel = moved.vel;
            dash = Some(dashing);
        } else {
            let flat = dashing.dir * dashing.exit_speed * free;
            vel = Vec3::new(flat.x, 0.0, flat.z);
            dash = None;
        }
    } else if slam.is_some() {
        let fall = body.slide(world, pos, Vec3::NEG_Y * tuning.slam_speed, dt, false);
        (pos, vel) = (fall.pos, fall.vel);
    } else if let Some(normal) = ground {
        let flat = if sliding {
            slide_move(horizontal(vel), wish, normal, tuning, dt)
        } else {
            ground_move(horizontal(vel), wish, tuning, dt)
        };
        let intended = along_ground(flat, normal);

        let start = pos;
        let plain = body.slide(world, start, intended, dt, true);
        (pos, vel) = (plain.pos, plain.vel);
        // Blocked by something steep: maybe it's a step we can walk up. Keep
        // whichever attempt got further.
        if plain.hit_wall
            && let Some(stepped) = body.step_up(world, start, intended, dt, tuning.step_height)
            && horizontal(stepped.pos - start).length_squared()
                > horizontal(plain.pos - start).length_squared() + 1e-3
        {
            (pos, vel) = (stepped.pos, stepped.vel);
        }
        // Stay glued to the ground walking down ramps and stairs.
        pos = body
            .snap_down(world, pos, tuning.step_height)
            .unwrap_or(pos);
    } else if let Some(contact) = wall {
        let normal = contact.plane.normal;
        match contact.kind {
            WallKind::Hang => {
                // Ease flush against the wall and hold still.
                pos = body
                    .slide(world, pos, -normal * WALL_STICK_SPEED, dt, false)
                    .pos;
                vel = Vec3::ZERO;
            }
            WallKind::Run => {
                vel = wall_run_velocity(vel, wish, normal, tuning, dt);
                // Split gravity like the air case below.
                vel.y -= tuning.wall_run_gravity * dt * 0.5;
                let run = body.slide(world, pos, vel - normal * WALL_STICK_SPEED, dt, false);
                pos = run.pos;
                // Drop the stick (and anything else heading into the wall). Contact
                // normals are never perfectly flat, so clipping can add a sliver of
                // speed; don't let it build up.
                let top = horizontal(vel).length();
                let flat = horizontal(clip(run.vel, normal)).clamp_length_max(top);
                vel = Vec3::new(flat.x, run.vel.y, flat.z);
                vel.y -= tuning.wall_run_gravity * dt * 0.5;
                // Runs sink slowly at most, so long ones don't end on the floor.
                vel.y = vel.y.max(-tuning.wall_run_fall_speed);
            }
        }
    } else {
        let flat = air_steer(horizontal(vel), wish, tuning.air_accel * dt);
        vel = Vec3::new(flat.x, vel.y, flat.z);
        // Half the gravity before the move and half after puts every tick exactly on
        // the true parabola, so jump height doesn't depend on the tick rate.
        vel.y -= tuning.gravity * dt * 0.5;
        let air = body.slide(world, pos, vel, dt, false);
        (pos, vel) = (air.pos, air.vel);
        vel.y -= tuning.gravity * dt * 0.5;
    }

    // However speed was gained, flat speed tops out here.
    let flat = horizontal(vel).clamp_length_max(tuning.max_speed);
    vel = Vec3::new(flat.x, vel.y, flat.z);

    pos = body.push_out(world, pos);
    let ground = body.find_ground(world, pos, vel);
    if let Some(normal) = ground {
        vel = along_ground(horizontal(vel), normal);
        wall = None;
        last_wall = None;
        air_jumps = tuning.air_jumps;
        wall_jumps = tuning.wall_jumps_per_airtime();
        if let Some(from) = slam.take() {
            slam_bounce = Some(slam_bounce_from(from - pos.y, tuning));
            vel = Vec3::ZERO;
        }
    }
    MovementState {
        position: pos,
        velocity: vel,
        on_ground: ground.is_some(),
        wall,
        last_wall,
        coyote,
        jump_buffer,
        air_jumps,
        wall_jumps,
        sliding: sliding && ground.is_some(),
        slide_cooldown,
        dash,
        stamina,
        slam,
        slam_bounce,
        grapple,
        grapple_armed,
    }
}

/// Throw, fly, stick or let go of the grapple. Returns it (if it's still out) and
/// whether the button is free to throw again.
fn update_grapple(
    state: &MovementState,
    input: &MoveInput,
    tuning: &MovementTuning,
    world: &impl CollisionWorld,
    eye: Vec3,
    dt: f32,
) -> (Option<Grapple>, bool) {
    if !input.grapple {
        return (None, true);
    }
    let mut armed = state.grapple_armed;
    let mut hook = match state.grapple {
        Some(hook) => hook,
        None if armed => {
            armed = false;
            let reach = aim_direction(input.yaw, input.pitch) * tuning.grapple_range;
            let (anchor, hooked) = match world.cast_ray(eye, reach) {
                Some(hit) => (eye + reach * hit.fraction, true),
                None => (eye + reach, false),
            };
            Grapple {
                anchor,
                hooked,
                reach: 0.0,
                attached: false,
            }
        }
        None => return (None, armed),
    };
    if hook.attached {
        // Arrived: let go and keep the momentum.
        if hook.anchor.distance(eye) <= tuning.grapple_release_distance {
            return (None, armed);
        }
    } else {
        hook.reach += tuning.grapple_hook_speed * dt;
        if hook.reach >= hook.anchor.distance(eye) {
            if !hook.hooked {
                return (None, armed); // missed: it reels back in
            }
            hook.attached = true;
        }
    }
    (Some(hook), armed)
}

/// What a slam bounce gives for landing a slam that fell `fallen` units.
fn slam_bounce_from(fallen: f32, tuning: &MovementTuning) -> SlamBounce {
    let height = (fallen.max(0.0) * tuning.slam_bounce_ratio).min(tuning.slam_bounce_max_height);
    SlamBounce {
        time_left: tuning.slam_bounce_time,
        speed: (2.0 * tuning.gravity * height).sqrt(),
    }
}

/// Use up a wall jump, unless they're unlimited.
fn spend_wall_jump(wall_jumps: &mut u32, tuning: &MovementTuning) {
    if tuning.wall_jumps.is_some() {
        *wall_jumps = wall_jumps.saturating_sub(1);
    }
}

/// Flat unit vector the player faces.
fn facing(yaw: f32) -> Vec3 {
    Vec3::new(-yaw.sin(), 0.0, -yaw.cos())
}

/// Unit vector the player looks along.
fn aim_direction(yaw: f32, pitch: f32) -> Vec3 {
    let (sin_pitch, cos_pitch) = pitch.sin_cos();
    facing(yaw) * cos_pitch + Vec3::Y * sin_pitch
}

/// Collision shape plus what counts as ground, shared by all the helpers.
struct Body {
    capsule: Capsule,
    min_ground_y: f32,
}

struct Ground {
    normal: Vec3,
    /// Height of the surface itself (not the feet).
    height: f32,
}

/// Result of checking for walls this tick.
#[derive(Default)]
struct WallUpdate {
    /// Running on or hanging from a wall.
    contact: Option<WallContact>,
    last_wall: Option<WallPlane>,
    /// A wall right beside us, grabbed or not. Jumping off it is a wall jump.
    touching: Option<WallPlane>,
}

struct Slide {
    pos: Vec3,
    vel: Vec3,
    /// Hit something too steep to stand on.
    hit_wall: bool,
}

impl Body {
    fn is_ground(&self, normal: Vec3) -> bool {
        normal.y >= self.min_ground_y
    }

    /// The walkable surface just below the feet, unless we're moving away from it.
    fn find_ground(&self, world: &impl CollisionWorld, pos: Vec3, vel: Vec3) -> Option<Vec3> {
        let hit = world.cast_capsule(&self.capsule, pos, Vec3::NEG_Y * GROUND_PROBE)?;
        let ground = self.ground_under(world, &hit)?;
        (vel.dot(ground.normal) <= LIFTOFF_SPEED).then_some(ground.normal)
    }

    /// The walkable surface the capsule touched, if it touched one.
    ///
    /// On an edge (a step lip, a ledge we're walking off) the rounded capsule's
    /// contact normal tilts even though the surface is flat, so we look straight
    /// down just past the contact, on the solid side, to see the real surface.
    fn ground_under(&self, world: &impl CollisionWorld, hit: &Hit) -> Option<Ground> {
        if hit.normal.y < EDGE_MIN_NORMAL_Y {
            return None;
        }
        let solid_side = -horizontal(hit.normal).normalize_or_zero();
        match world.cast_ray(hit.point + solid_side * 0.25 + Vec3::Y, Vec3::NEG_Y * 2.0) {
            Some(ray) if self.is_ground(ray.normal) => Some(Ground {
                normal: ray.normal,
                height: ray.point.y,
            }),
            _ if self.is_ground(hit.normal) => Some(Ground {
                normal: hit.normal,
                height: hit.point.y,
            }),
            _ => None,
        }
    }

    /// Restore the SKIN gap if rounding or a bad spawn left us touching something.
    fn push_out(&self, world: &impl CollisionWorld, pos: Vec3) -> Vec3 {
        pos + world.push_out(&self.capsule, pos, SKIN)
    }

    /// Move for `dt` seconds, sliding along whatever is in the way.
    ///
    /// With `grounded`, surfaces too steep to stand on act as vertical walls, so
    /// walking into a steep slope blocks you instead of sliding you up it.
    fn slide(
        &self,
        world: &impl CollisionWorld,
        mut pos: Vec3,
        mut vel: Vec3,
        dt: f32,
        grounded: bool,
    ) -> Slide {
        let mut remaining = dt;
        let mut planes = Vec::with_capacity(MAX_BUMPS);
        let mut hit_wall = false;

        for _ in 0..MAX_BUMPS {
            let motion = vel * remaining;
            let len = motion.length();
            if len < 1e-4 {
                break;
            }
            let Some(hit) = world.cast_capsule(&self.capsule, pos, motion) else {
                pos += motion;
                break;
            };
            let dir = motion / len;
            // Stop SKIN short of the surface, measured along its normal. At a grazing
            // angle that can mean not moving at all this bump, which is fine: the
            // velocity gets clipped and the next bump slides along the surface.
            let into = -dir.dot(hit.normal);
            let back_off = if into > 1e-3 { SKIN / into } else { SKIN };
            let travel = (hit.fraction * len - back_off).max(0.0);
            pos += dir * travel;
            remaining *= 1.0 - travel / len;

            let mut normal = hit.normal;
            if !self.is_ground(normal) {
                hit_wall = true;
                if grounded {
                    normal = horizontal(normal).try_normalize().unwrap_or(normal);
                }
            }
            planes.push(normal);
            vel = clip_to_planes(vel, &planes);
            if vel == Vec3::ZERO {
                break;
            }
        }
        Slide { pos, vel, hit_wall }
    }

    /// Try the move again from `step_height` higher, then drop back down. Returns
    /// `None` if there's no headroom or nothing walkable to land on.
    fn step_up(
        &self,
        world: &impl CollisionWorld,
        pos: Vec3,
        vel: Vec3,
        dt: f32,
        step_height: f32,
    ) -> Option<Slide> {
        let lift = self.sweep_distance(world, pos, Vec3::Y, step_height);
        if lift < 1.0 {
            return None;
        }
        let across = self.slide(world, pos + Vec3::Y * lift, horizontal(vel), dt, true);

        let hit = world.cast_capsule(&self.capsule, across.pos, Vec3::NEG_Y * (lift + SKIN))?;
        // Check the surface, not the feet: landing on the lip of something taller
        // would otherwise ratchet us up it a step_height at a time.
        if self.ground_under(world, &hit)?.height > pos.y + step_height {
            return None;
        }
        let drop = (hit.fraction * (lift + SKIN) - SKIN).max(0.0);
        Some(Slide {
            pos: across.pos - Vec3::Y * drop,
            vel: across.vel,
            hit_wall: false,
        })
    }

    /// If there's ground within `max_drop` below, move down onto it.
    fn snap_down(&self, world: &impl CollisionWorld, pos: Vec3, max_drop: f32) -> Option<Vec3> {
        let hit = world.cast_capsule(&self.capsule, pos, Vec3::NEG_Y * max_drop)?;
        self.ground_under(world, &hit)?;
        Some(pos - Vec3::Y * (hit.fraction * max_drop - SKIN).max(0.0))
    }

    /// No walkable ground within `height` below the feet.
    fn clear_of_ground(&self, world: &impl CollisionWorld, pos: Vec3, height: f32) -> bool {
        match world.cast_capsule(&self.capsule, pos, Vec3::NEG_Y * height) {
            Some(hit) => self.ground_under(world, &hit).is_none(),
            None => true,
        }
    }

    /// The nearest wall within WALL_PROBE of the capsule, looking in eight
    /// directions plus towards `hint` (the wall we're already on).
    fn find_wall(
        &self,
        world: &impl CollisionWorld,
        pos: Vec3,
        hint: Option<Vec3>,
    ) -> Option<WallPlane> {
        let around = (0..8).map(|k| {
            let (sin, cos) = (k as f32 * FRAC_PI_4).sin_cos();
            Vec3::new(cos, 0.0, sin)
        });
        let mut best: Option<(f32, WallPlane)> = None;
        for dir in hint.map(|n| -n).into_iter().chain(around) {
            let Some(hit) = world.cast_capsule(&self.capsule, pos, dir * WALL_PROBE) else {
                continue;
            };
            if hit.normal.y.abs() > WALL_MAX_NORMAL_Y {
                continue;
            }
            // The capsule might only be clipping a corner or a low lip. Check with a
            // ray from the body's middle, which also gives the face normal rather
            // than an edge's tilted one.
            let towards = -horizontal(hit.normal).normalize_or_zero();
            let reach = self.capsule.radius + WALL_PROBE + 1.0;
            let centre = pos + self.capsule.center_offset();
            let Some(ray) = world.cast_ray(centre, towards * reach) else {
                continue;
            };
            let Some(normal) = horizontal(ray.normal).try_normalize() else {
                continue;
            };
            if ray.normal.y.abs() > WALL_MAX_NORMAL_Y {
                continue;
            }
            if best.is_none_or(|(fraction, _)| hit.fraction < fraction) {
                let offset = normal.dot(ray.point);
                best = Some((hit.fraction, WallPlane { normal, offset }));
            }
        }
        best.map(|(_, plane)| plane)
    }

    /// Whether we're on a wall this tick: keep, start, switch or let go of a
    /// wall-run or hang.
    fn next_wall(
        &self,
        world: &impl CollisionWorld,
        state: &MovementState,
        input: &MoveInput,
        tuning: &MovementTuning,
        pos: Vec3,
        dt: f32,
    ) -> WallUpdate {
        let found = self.find_wall(world, pos, state.wall.map(|w| w.plane.normal));
        let wish = wish_velocity(input, tuning).normalize_or_zero();
        // A run needs speed along the wall, and the stick not pointing away from it.
        let can_run = |plane: &WallPlane| {
            along_wall(state.velocity, plane.normal).length() >= tuning.wall_run_min_speed
                && wish.dot(plane.normal) <= WALL_LEAVE_DOT
        };
        let contact = |kind, plane, time| WallContact { kind, plane, time };

        if let Some(current) = state.wall {
            let time = current.time + dt;
            let next = found.and_then(|plane| match current.kind {
                WallKind::Hang => (input.hang && time <= tuning.wall_hang_max_time)
                    .then(|| contact(WallKind::Hang, plane, time)),
                // Hang mid-run: stop dead, with a fresh hang timer.
                WallKind::Run if input.hang => Some(contact(WallKind::Hang, plane, 0.0)),
                WallKind::Run => (time <= tuning.wall_run_max_time && can_run(&plane))
                    .then(|| contact(WallKind::Run, plane, time)),
            });
            return match next {
                Some(next) => WallUpdate {
                    contact: Some(next),
                    last_wall: state.last_wall,
                    touching: found,
                },
                None => WallUpdate {
                    contact: None,
                    last_wall: Some(current.plane),
                    touching: found,
                },
            };
        }

        let Some(plane) = found else {
            return WallUpdate {
                last_wall: state.last_wall,
                ..WallUpdate::default()
            };
        };
        let grabbable = !state.last_wall.is_some_and(|last| last.same_as(&plane))
            && self.clear_of_ground(world, pos, tuning.wall_min_height);
        let kind = if !grabbable {
            None
        } else if input.hang {
            Some(WallKind::Hang)
        } else if can_run(&plane) {
            Some(WallKind::Run)
        } else {
            None
        };
        WallUpdate {
            contact: kind.map(|kind| contact(kind, plane, 0.0)),
            last_wall: state.last_wall,
            touching: Some(plane),
        }
    }

    /// How far we can move in `dir` (unit vector), up to `max`, keeping SKIN clear.
    fn sweep_distance(&self, world: &impl CollisionWorld, pos: Vec3, dir: Vec3, max: f32) -> f32 {
        match world.cast_capsule(&self.capsule, pos, dir * max) {
            Some(hit) => (hit.fraction * max - SKIN).max(0.0),
            None => max,
        }
    }
}

/// Direction and speed the player is asking for, flat on the ground plane.
fn wish_velocity(input: &MoveInput, tuning: &MovementTuning) -> Vec3 {
    let wish = input.wish.clamp_length_max(1.0);
    let (sin, cos) = input.yaw.sin_cos();
    let forward = Vec3::new(-sin, 0.0, -cos);
    let right = Vec3::new(cos, 0.0, -sin);
    let speed = if input.sprint && wish.y > 0.0 {
        tuning.sprint_speed
    } else {
        tuning.walk_speed
    };
    (right * wish.x + forward * wish.y) * speed
}

/// Ground control for one tick, split into the part of the motion along the stick
/// and the part across it.
///
/// - Across: always removed at `ground_accel`, so turning doesn't drift.
/// - Along: approaches the wished-for speed at `ground_accel`. Extra speed (from
///   landing out of a jump or wall-run) only bleeds off at `overspeed_decel`, so
///   it carries from move to move as long as you keep pushing that way.
/// - No input: stop at `ground_friction`.
fn ground_move(current: Vec3, wish: Vec3, tuning: &MovementTuning, dt: f32) -> Vec3 {
    if wish == Vec3::ZERO {
        return move_towards(current, Vec3::ZERO, tuning.ground_friction * dt);
    }
    let dir = wish.normalize();
    let top = wish.length();
    let along = current.dot(dir);
    let across = move_towards(current - dir * along, Vec3::ZERO, tuning.ground_accel * dt);
    let along = if along > top {
        (along - tuning.overspeed_decel * dt).max(top)
    } else {
        (along + tuning.ground_accel * dt).min(top)
    };
    dir * along + across
}

/// Sliding for one tick: low friction, gentle steering that never adds speed, and
/// slopes that speed you up going down (and slow you going up).
fn slide_move(current: Vec3, wish: Vec3, normal: Vec3, tuning: &MovementTuning, dt: f32) -> Vec3 {
    let slowed = move_towards(current, Vec3::ZERO, tuning.slide_friction * dt);
    let steered = match wish.try_normalize() {
        Some(dir) => (slowed + dir * tuning.slide_steer * dt).clamp_length_max(slowed.length()),
        None => slowed,
    };
    // Gravity along the slope is g·sinθ; its flat part is g·sinθ·cosθ, downhill.
    steered + horizontal(normal) * normal.y * tuning.gravity * dt
}

/// Double jump: launch upwards at the air-jump speed (replacing any fall) and
/// turn the flat motion towards the stick by `air_jump_redirect`.
fn air_jump(vel: Vec3, wish: Vec3, tuning: &MovementTuning) -> Vec3 {
    let flat = horizontal(vel);
    let turned = match wish.try_normalize() {
        Some(dir) => {
            let target = dir * flat.length().max(wish.length());
            flat.lerp(target, tuning.air_jump_redirect.clamp(0.0, 1.0))
        }
        None => flat,
    };
    turned + Vec3::Y * tuning.air_jump_speed()
}

/// Wall-run velocity for one tick: along the wall in the direction we're already
/// going. Speeds up to `wall_run_speed` while the stick points that way and slows
/// down when it doesn't. A fast entry above the cap keeps its speed.
fn wall_run_velocity(
    vel: Vec3,
    wish: Vec3,
    normal: Vec3,
    tuning: &MovementTuning,
    dt: f32,
) -> Vec3 {
    let along = along_wall(vel, normal);
    let dir = along.normalize_or_zero();
    let speed = along.length();
    let rate = tuning.wall_run_accel * dt;
    let speed = if wish.dot(dir) <= 0.0 {
        (speed - rate).max(0.0)
    } else if speed < tuning.wall_run_speed {
        (speed + rate).min(tuning.wall_run_speed)
    } else {
        speed
    };
    dir * speed + Vec3::Y * vel.y
}

/// A jump on a wall is a hop (stay on it) unless the stick points away from it.
/// With no stick at all, it's a kick-off.
fn is_wall_hop(wish: Vec3, normal: Vec3) -> bool {
    wish.try_normalize()
        .is_some_and(|dir| dir.dot(normal) < WALL_KICK_DOT)
}

/// Off the wall: keep the speed along it, push away from it, and jump.
fn wall_jump(vel: Vec3, normal: Vec3, tuning: &MovementTuning) -> Vec3 {
    along_wall(vel, normal) + normal * tuning.wall_jump_push + Vec3::Y * tuning.jump_speed()
}

/// The flat part of `v` that runs along a wall with horizontal `normal`.
fn along_wall(v: Vec3, normal: Vec3) -> Vec3 {
    let flat = horizontal(v);
    flat - normal * flat.dot(normal)
}

/// Air control: steer towards `wish` without ever gaining speed beyond the faster
/// of the current speed and the wished-for speed.
fn air_steer(current: Vec3, wish: Vec3, accel: f32) -> Vec3 {
    if wish == Vec3::ZERO {
        return current;
    }
    let cap = current.length().max(wish.length());
    (current + wish.normalize() * accel).clamp_length_max(cap)
}

fn horizontal(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Tilt a flat velocity to run along a slope, keeping its horizontal part, so
/// ramps don't change how fast you cover ground.
fn along_ground(flat: Vec3, normal: Vec3) -> Vec3 {
    flat - Vec3::Y * (flat.dot(normal) / normal.y)
}

fn move_towards(current: Vec3, target: Vec3, max_delta: f32) -> Vec3 {
    let delta = target - current;
    let dist = delta.length();
    if dist <= max_delta || dist < 1e-6 {
        target
    } else {
        current + delta / dist * max_delta
    }
}

/// Remove only the part of `v` that pushes into `normal`.
fn clip(v: Vec3, normal: Vec3) -> Vec3 {
    v - normal * v.dot(normal).min(0.0)
}

/// Slide along the surfaces hit this tick. If one surface works, use it; if we're
/// wedged between two (a corner, a wall meeting a ramp), follow their crease; if
/// three or more box us in, stop.
fn clip_to_planes(vel: Vec3, planes: &[Vec3]) -> Vec3 {
    let pushes_into = |v: Vec3, skip: &[usize]| {
        planes
            .iter()
            .enumerate()
            .any(|(k, p)| !skip.contains(&k) && v.dot(*p) < -1e-3)
    };
    for (i, &plane) in planes.iter().enumerate().rev() {
        let v = clip(vel, plane);
        if !pushes_into(v, &[i]) {
            return v;
        }
    }
    for i in 0..planes.len() {
        for j in i + 1..planes.len() {
            let crease = planes[i].cross(planes[j]).normalize_or_zero();
            let v = crease * vel.dot(crease);
            if !pushes_into(v, &[i, j]) {
                return v;
            }
        }
    }
    Vec3::ZERO
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn along_ground_keeps_horizontal_speed_and_follows_the_slope() {
        let ramp = Vec3::new(0.0, 1.0, 1.0).normalize();
        let v = along_ground(Vec3::new(0.0, 0.0, -100.0), ramp);
        assert!(v.dot(ramp).abs() < 1e-4);
        assert!((horizontal(v).length() - 100.0).abs() < 1e-3);
        assert!(v.y > 0.0, "walking up the ramp should climb: {v}");
    }

    #[test]
    fn crease_between_two_walls() {
        let a = Vec3::X;
        let b = Vec3::Z;
        // Pushing diagonally into a corner: nothing left once both walls are clipped.
        assert_eq!(
            clip_to_planes(Vec3::new(-1.0, 0.0, -1.0), &[a, b]),
            Vec3::ZERO
        );
        // Pushing into the corner while moving up slides up the crease.
        let v = clip_to_planes(Vec3::new(-1.0, 1.0, -1.0), &[a, b]);
        assert!(v.abs_diff_eq(Vec3::Y, 1e-4), "{v}");
    }

    #[test]
    fn wall_jump_keeps_speed_along_the_wall_and_pushes_off() {
        let tuning: MovementTuning = ron::from_str(include_str!("../tests/tuning.ron")).unwrap();
        // Wall on the right (normal -X), running towards -Z, drifting into it.
        let v = wall_jump(Vec3::new(50.0, -20.0, -400.0), Vec3::NEG_X, &tuning);
        assert!((v.z + 400.0).abs() < 1e-3, "{v}");
        assert!((v.x + tuning.wall_jump_push).abs() < 1e-3, "{v}");
        assert!((v.y - tuning.jump_speed()).abs() < 1e-3, "{v}");
    }

    #[test]
    fn landing_fast_only_sheds_overspeed_slowly() {
        let tuning: MovementTuning = ron::from_str(include_str!("../tests/tuning.ron")).unwrap();
        let dt = 1.0 / 60.0;
        let wish = Vec3::new(0.0, 0.0, -tuning.walk_speed);
        let v = ground_move(Vec3::new(0.0, 0.0, -500.0), wish, &tuning, dt);
        let expected = 500.0 - tuning.overspeed_decel * dt;
        assert!((v.length() - expected).abs() < 1e-3, "{v}");
        // Steering against the motion still brakes hard.
        let v = ground_move(Vec3::new(0.0, 0.0, -500.0), -wish, &tuning, dt);
        assert!(v.length() < 500.0 - tuning.ground_accel * dt + 1e-3, "{v}");
    }

    #[test]
    fn turning_on_the_ground_kills_sideways_drift() {
        let tuning: MovementTuning = ron::from_str(include_str!("../tests/tuning.ron")).unwrap();
        let dt = 1.0 / 60.0;
        // Running at top speed towards -Z, then the stick swings to +X.
        let mut v = Vec3::new(0.0, 0.0, -tuning.walk_speed);
        let wish = Vec3::new(tuning.walk_speed, 0.0, 0.0);
        let ticks = (tuning.walk_speed / (tuning.ground_accel * dt)).ceil() as usize;
        for _ in 0..ticks {
            v = ground_move(v, wish, &tuning, dt);
        }
        assert!(v.abs_diff_eq(wish, 1e-3), "{v}");
    }

    #[test]
    fn air_jump_turns_towards_the_stick_and_keeps_speed() {
        let tuning: MovementTuning = ron::from_str(include_str!("../tests/tuning.ron")).unwrap();
        let wish = Vec3::new(tuning.walk_speed, 0.0, 0.0);
        let v = air_jump(Vec3::new(0.0, -300.0, -400.0), wish, &tuning);
        assert!((v.y - tuning.air_jump_speed()).abs() < 1e-3, "{v}");
        // Test tuning redirects fully: all 400 units/s now go towards +X.
        assert!(v.abs_diff_eq(Vec3::new(400.0, v.y, 0.0), 1e-3), "{v}");
        // No stick: straight up, flat motion unchanged.
        let v = air_jump(Vec3::new(0.0, -300.0, -400.0), Vec3::ZERO, &tuning);
        assert!((v.z + 400.0).abs() < 1e-3 && v.x == 0.0, "{v}");
    }

    #[test]
    fn air_steer_never_adds_speed() {
        let v = air_steer(
            Vec3::new(300.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -100.0),
            50.0,
        );
        assert!(v.length() <= 300.0 + 1e-3);
        assert!(v.z < 0.0);
    }
}
