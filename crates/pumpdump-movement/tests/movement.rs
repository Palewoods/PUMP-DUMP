//! Behaviour tests: run the real `step` function tick by tick in small hand-built
//! worlds. No Bevy, no game data.

use glam::{Quat, Vec2, Vec3};
use pumpdump_movement::{MoveInput, MovementState, MovementTuning, StaticWorld, WallKind, step};

const DT: f32 = 1.0 / 60.0;
const RADIUS: f32 = 16.0;

/// Test values (tests/tuning.ron), not the sandbox's placeholders, so tests don't
/// break when tuning changes. Written as RON to also check the format parses.
fn tuning() -> MovementTuning {
    ron::from_str(include_str!("tuning.ron")).unwrap()
}

/// Flat floor with its top at y = 0.
fn floor() -> StaticWorld {
    let mut world = StaticWorld::new();
    world.add_box(
        Vec3::new(0.0, -10.0, 0.0),
        Vec3::new(5000.0, 10.0, 5000.0),
        Quat::IDENTITY,
    );
    world
}

/// Box from `min` to `max` corners.
fn add_block(world: &mut StaticWorld, min: Vec3, max: Vec3) {
    world.add_box((min + max) * 0.5, (max - min) * 0.5, Quat::IDENTITY);
}

/// A wedge rising towards -Z, starting at `z_start`, `run` long, at `degrees`.
fn add_ramp(world: &mut StaticWorld, z_start: f32, run: f32, degrees: f32) {
    let rise = run * degrees.to_radians().tan();
    let z_end = z_start - run;
    let pts = [
        Vec3::new(-300.0, 0.0, z_start),
        Vec3::new(300.0, 0.0, z_start),
        Vec3::new(-300.0, 0.0, z_end),
        Vec3::new(300.0, 0.0, z_end),
        Vec3::new(-300.0, rise, z_end),
        Vec3::new(300.0, rise, z_end),
    ];
    assert!(world.add_convex(&pts));
}

const FORWARD: MoveInput = MoveInput {
    wish: Vec2::new(0.0, 1.0),
    yaw: 0.0,
    jump: false,
    sprint: false,
    hang: false,
    slide: false,
};

/// Run `seconds` of the same input, calling `each` after every tick.
fn run(
    world: &StaticWorld,
    mut state: MovementState,
    input: MoveInput,
    seconds: f32,
    mut each: impl FnMut(&MovementState),
) -> MovementState {
    let t = tuning();
    for _ in 0..(seconds / DT).round() as usize {
        state = step(&state, &input, &t, world, DT);
        each(&state);
    }
    state
}

fn settle(world: &StaticWorld, at: Vec3) -> MovementState {
    let state = run(
        world,
        MovementState::new(at),
        MoveInput::default(),
        0.5,
        |_| {},
    );
    assert!(state.on_ground, "didn't land: {state:?}");
    state
}

fn speed(state: &MovementState) -> f32 {
    Vec2::new(state.velocity.x, state.velocity.z).length()
}

// ---- walk / sprint ----

#[test]
fn walk_and_sprint_reach_their_top_speeds() {
    let world = floor();
    let start = settle(&world, Vec3::ZERO);

    let walked = run(&world, start, FORWARD, 1.0, |_| {});
    assert!((speed(&walked) - 200.0).abs() < 0.01, "{walked:?}");

    let sprinted = run(
        &world,
        start,
        MoveInput {
            sprint: true,
            ..FORWARD
        },
        1.0,
        |_| {},
    );
    assert!((speed(&sprinted) - 300.0).abs() < 0.01, "{sprinted:?}");

    // Sprint only counts going forwards.
    let back = MoveInput {
        wish: Vec2::new(0.0, -1.0),
        sprint: true,
        ..FORWARD
    };
    let backed = run(&world, start, back, 1.0, |_| {});
    assert!((speed(&backed) - 200.0).abs() < 0.01, "{backed:?}");
}

#[test]
fn friction_stops_you() {
    let world = floor();
    let moving = run(&world, settle(&world, Vec3::ZERO), FORWARD, 1.0, |_| {});
    let stopped = run(&world, moving, MoveInput::default(), 0.5, |_| {});
    assert_eq!(stopped.velocity, Vec3::ZERO);
}

#[test]
fn yaw_turns_the_wish_direction() {
    let world = floor();
    let left = MoveInput {
        yaw: std::f32::consts::FRAC_PI_2,
        ..FORWARD
    };
    let state = run(&world, settle(&world, Vec3::ZERO), left, 1.0, |_| {});
    // Facing -Z and turning left by 90° faces -X.
    assert!(
        state.position.x < -100.0 && state.position.z.abs() < 1e-3,
        "{state:?}"
    );
}

// ---- jump / gravity ----

fn jump_apex(dt: f32) -> f32 {
    let world = floor();
    let t = tuning();
    let mut state = settle(&world, Vec3::ZERO);
    let ground_y = state.position.y;
    state = step(
        &state,
        &MoveInput {
            jump: true,
            ..Default::default()
        },
        &t,
        &world,
        dt,
    );
    let mut apex = state.position.y;
    for _ in 0..(2.0 / dt) as usize {
        state = step(&state, &MoveInput::default(), &t, &world, dt);
        apex = apex.max(state.position.y);
    }
    assert!(state.on_ground, "should land again: {state:?}");
    assert!((state.position.y - ground_y).abs() < 0.2, "{state:?}");
    apex - ground_y
}

#[test]
fn jump_apex_height_matches_tuning() {
    let apex = jump_apex(DT);
    assert!((apex - 50.0).abs() < 0.2, "apex {apex}");
}

#[test]
fn jump_height_does_not_depend_on_tick_rate() {
    for hz in [30.0, 60.0, 120.0, 144.0] {
        let apex = jump_apex(1.0 / hz);
        assert!((apex - 50.0).abs() < 0.2, "{hz} Hz: apex {apex}");
    }
}

#[test]
fn falls_under_gravity() {
    let world = floor();
    let t = tuning();
    let mut state = MovementState::new(Vec3::new(0.0, 500.0, 0.0));
    for _ in 0..30 {
        state = step(&state, &MoveInput::default(), &t, &world, DT);
    }
    // After 0.5 s: fallen g t² / 2 = 100 units, moving g t = 400 units/s.
    assert!((state.position.y - 400.0).abs() < 0.01, "{state:?}");
    assert!((state.velocity.y + 400.0).abs() < 0.01, "{state:?}");
    assert!(!state.on_ground);
}

#[test]
fn cannot_jump_in_the_air_without_an_air_jump_left() {
    let world = floor();
    let t = tuning();
    let mut state = MovementState::new(Vec3::new(0.0, 500.0, 0.0));
    state = step(
        &state,
        &MoveInput {
            jump: true,
            ..Default::default()
        },
        &t,
        &world,
        DT,
    );
    assert!(state.velocity.y < 0.0, "{state:?}");
}

// ---- walls ----

/// A thin wall whose front face is at z = -199.
fn wall() -> StaticWorld {
    let mut world = floor();
    add_block(
        &mut world,
        Vec3::new(-500.0, 0.0, -201.0),
        Vec3::new(500.0, 300.0, -199.0),
    );
    world
}
const WALL_LIMIT: f32 = -199.0 + RADIUS;

#[test]
fn walking_into_a_wall_stops_at_it() {
    let world = wall();
    let state = run(
        &world,
        settle(&world, Vec3::ZERO),
        MoveInput {
            sprint: true,
            ..FORWARD
        },
        3.0,
        |s| {
            assert!(s.position.z >= WALL_LIMIT, "went into the wall: {s:?}");
        },
    );
    assert!(
        state.position.z < WALL_LIMIT + 1.0,
        "should be touching the wall: {state:?}"
    );
}

#[test]
fn very_fast_movement_cannot_tunnel_through_a_wall() {
    let world = wall();
    // 50,000 units/s covers 833 units in one tick, far past the 2-unit-thick wall.
    let mut state = MovementState::new(Vec3::new(0.0, 50.0, 0.0));
    state.velocity = Vec3::new(0.0, 0.0, -50_000.0);
    let state = step(&state, &MoveInput::default(), &tuning(), &world, DT);
    assert!(state.position.z >= WALL_LIMIT, "{state:?}");
    assert!(
        state.velocity.z.abs() < 1e-3,
        "speed into the wall should be removed: {state:?}"
    );
}

#[test]
fn moving_diagonally_into_a_wall_slides_along_it() {
    let world = wall();
    let diagonal = MoveInput {
        wish: Vec2::new(1.0, 1.0).normalize(),
        ..FORWARD
    };
    let state = run(&world, settle(&world, Vec3::ZERO), diagonal, 3.0, |s| {
        assert!(s.position.z >= WALL_LIMIT, "{s:?}");
    });
    assert!(
        state.position.x > 250.0,
        "should keep sliding sideways: {state:?}"
    );
}

#[test]
fn corners_stop_you_without_jitter() {
    let mut world = wall();
    // Second wall on the right, face at x = 99.
    add_block(
        &mut world,
        Vec3::new(99.0, 0.0, -500.0),
        Vec3::new(101.0, 300.0, 500.0),
    );
    let into_corner = MoveInput {
        wish: Vec2::new(1.0, 1.0).normalize(),
        ..FORWARD
    };
    let wedged = run(&world, settle(&world, Vec3::ZERO), into_corner, 3.0, |s| {
        assert!(
            s.position.z >= WALL_LIMIT && s.position.x <= 99.0 - RADIUS,
            "{s:?}"
        );
    });
    // Keep pushing for another second: we shouldn't move or vibrate.
    run(&world, wedged, into_corner, 1.0, |s| {
        assert!(
            s.position.distance(wedged.position) < 0.01,
            "{s:?} vs {wedged:?}"
        );
    });
}

// ---- slopes ----

#[test]
fn walks_up_a_gentle_ramp() {
    let mut world = floor();
    add_ramp(&mut world, -100.0, 400.0, 30.0);
    let state = run(&world, settle(&world, Vec3::ZERO), FORWARD, 2.0, |_| {});
    assert!(state.on_ground, "{state:?}");
    assert!(state.position.y > 100.0, "should have climbed: {state:?}");
}

#[test]
fn cannot_walk_up_a_steep_ramp() {
    let mut world = floor();
    add_ramp(&mut world, -100.0, 200.0, 60.0);
    let mut max_y = 0.0f32;
    run(
        &world,
        settle(&world, Vec3::ZERO),
        MoveInput {
            sprint: true,
            ..FORWARD
        },
        3.0,
        |s| {
            max_y = max_y.max(s.position.y);
        },
    );
    assert!(max_y < 20.0, "climbed a 60° slope to {max_y}");
}

#[test]
fn slides_down_a_steep_ramp() {
    let mut world = floor();
    add_ramp(&mut world, -100.0, 200.0, 60.0);
    // Dropped onto the middle of the slope with no input.
    let state = run(
        &world,
        MovementState::new(Vec3::new(0.0, 250.0, -200.0)),
        MoveInput::default(),
        3.0,
        |_| {},
    );
    assert!(state.on_ground && state.position.y < 1.0, "{state:?}");
    assert!(
        state.position.z > -100.0,
        "should end up off the bottom of the slope: {state:?}"
    );
}

// ---- steps ----

fn walk_at_step(height: f32) -> MovementState {
    let mut world = floor();
    add_block(
        &mut world,
        Vec3::new(-500.0, 0.0, -600.0),
        Vec3::new(500.0, height, -100.0),
    );
    run(&world, settle(&world, Vec3::ZERO), FORWARD, 2.0, |_| {})
}

#[test]
fn walks_up_a_step_below_step_height() {
    let state = walk_at_step(12.0);
    assert!(state.on_ground, "{state:?}");
    assert!((state.position.y - 12.0).abs() < 0.5, "{state:?}");
    assert!(state.position.z < -200.0, "{state:?}");
}

#[test]
fn walks_up_a_step_just_under_step_height() {
    let state = walk_at_step(17.5);
    assert!((state.position.y - 17.5).abs() < 0.5, "{state:?}");
}

#[test]
fn blocked_by_a_step_above_step_height() {
    let state = walk_at_step(24.0);
    assert!(state.position.y < 1.0, "{state:?}");
    assert!(state.position.z >= -100.0 + RADIUS - 0.5, "{state:?}");
}

#[test]
fn walks_down_stairs_without_falling() {
    // Four 12-unit steps down, each 32 deep. Player starts on the top one.
    let mut world = floor();
    for k in 0..4 {
        let top = 12.0 * (4 - k) as f32;
        add_block(
            &mut world,
            Vec3::new(-500.0, 0.0, -32.0 * k as f32),
            Vec3::new(500.0, top, 300.0),
        );
    }
    let mut longest_airborne = 0;
    let mut airborne = 0;
    let state = run(
        &world,
        settle(&world, Vec3::new(0.0, 48.0, 100.0)),
        FORWARD,
        2.0,
        |s| {
            airborne = if s.on_ground { 0 } else { airborne + 1 };
            longest_airborne = longest_airborne.max(airborne);
        },
    );
    assert!(state.on_ground && state.position.y < 0.5, "{state:?}");
    assert!(
        longest_airborne <= 2,
        "airborne for {longest_airborne} ticks in a row"
    );
}

#[test]
fn walking_off_a_ledge_falls() {
    let mut world = floor();
    add_block(
        &mut world,
        Vec3::new(-500.0, 0.0, -100.0),
        Vec3::new(500.0, 200.0, 300.0),
    );
    let mut was_airborne = false;
    let state = run(
        &world,
        settle(&world, Vec3::new(0.0, 200.0, 0.0)),
        FORWARD,
        3.0,
        |s| {
            was_airborne |= !s.on_ground;
        },
    );
    assert!(was_airborne);
    assert!(state.on_ground && state.position.y < 0.5, "{state:?}");
}

// ---- jump timing ----

#[test]
fn coyote_time_lets_you_jump_just_after_a_ledge() {
    let mut world = floor();
    add_block(
        &mut world,
        Vec3::new(-500.0, 0.0, -100.0),
        Vec3::new(500.0, 200.0, 300.0),
    );
    let t = tuning();
    let jump = MoveInput {
        jump: true,
        ..FORWARD
    };
    let mut state = settle(&world, Vec3::new(0.0, 200.0, 0.0));
    while state.on_ground {
        state = step(&state, &FORWARD, &t, &world, DT);
    }

    // Three ticks (0.05 s) after leaving the edge: still in the window.
    let mut late = state;
    for _ in 0..3 {
        late = step(&late, &FORWARD, &t, &world, DT);
    }
    let jumped = step(&late, &jump, &t, &world, DT);
    assert!(jumped.velocity.y > 0.0, "{jumped:?}");
    assert_eq!(jumped.air_jumps, 1, "a ground jump, not the double jump");

    // Twelve ticks (0.2 s): too late, so the press spends the double jump.
    let mut too_late = state;
    for _ in 0..12 {
        too_late = step(&too_late, &FORWARD, &t, &world, DT);
    }
    let doubled = step(&too_late, &jump, &t, &world, DT);
    assert_eq!(doubled.air_jumps, 0, "{doubled:?}");
}

/// Drop from 100 up, press jump once when the feet are `press_at` above the floor,
/// and report whether we ever rise again.
fn jumps_after_landing(press_at: f32) -> bool {
    let world = floor();
    let t = tuning();
    let mut state = MovementState::new(Vec3::new(0.0, 100.0, 0.0));
    let mut pressed = false;
    let mut landed = false;
    for _ in 0..60 {
        let press = !pressed && state.position.y < press_at;
        pressed |= press;
        let input = MoveInput {
            jump: press,
            ..Default::default()
        };
        state = step(&state, &input, &t, &world, DT);
        landed |= state.on_ground;
        if landed && state.velocity.y > 0.0 {
            return true;
        }
    }
    false
}

#[test]
fn jump_pressed_just_before_landing_still_jumps() {
    // Falling at ~350 units/s, 10 units is about 2 ticks before touchdown.
    assert!(jumps_after_landing(10.0));
    // 80 units up is far outside the 0.1 s buffer.
    assert!(!jumps_after_landing(80.0));
}

#[test]
fn landing_keeps_most_of_your_speed() {
    let world = floor();
    let t = tuning();
    let mut state = MovementState::new(Vec3::new(0.0, 30.0, 0.0));
    state.velocity = Vec3::new(0.0, 0.0, -500.0);
    let sprint = MoveInput {
        sprint: true,
        ..FORWARD
    };
    while !state.on_ground {
        state = step(&state, &sprint, &t, &world, DT);
    }
    let landed = run(&world, state, sprint, 0.1, |_| {});
    assert!(speed(&landed) > 450.0, "{landed:?}");
    // But it does settle back to sprint speed.
    let settled = run(&world, landed, sprint, 2.0, |_| {});
    assert!((speed(&settled) - 300.0).abs() < 0.01, "{settled:?}");
}

// ---- wall-run / wall-hang ----

/// A tall wall whose face is at x = 100, facing -X, running along Z.
fn side_wall() -> StaticWorld {
    let mut world = floor();
    add_block(
        &mut world,
        Vec3::new(100.0, 0.0, -5000.0),
        Vec3::new(120.0, 1000.0, 1000.0),
    );
    world
}
const SIDE_WALL_LIMIT: f32 = 100.0 - RADIUS;

/// In the air beside `side_wall`, `height` up, running along it towards -Z.
fn beside_wall(height: f32) -> MovementState {
    let mut state = MovementState::new(Vec3::new(SIDE_WALL_LIMIT - 2.0, height, 0.0));
    state.velocity = Vec3::new(0.0, 0.0, -300.0);
    // As if we'd jumped from the ground to get here.
    state.wall_jumps = 3;
    state
}

const SPRINT: MoveInput = MoveInput {
    sprint: true,
    ..FORWARD
};

#[test]
fn running_along_a_wall_in_the_air_wall_runs() {
    let world = side_wall();
    let start = beside_wall(300.0);
    let state = run(&world, start, SPRINT, 1.0, |s| {
        assert_eq!(s.wall.map(|w| w.kind), Some(WallKind::Run), "{s:?}");
        assert!(s.position.x <= SIDE_WALL_LIMIT, "{s:?}");
    });
    // Free fall would drop 400 in a second; wall-run gravity only 75.
    let dropped = start.position.y - state.position.y;
    assert!((dropped - 75.0).abs() < 1.0, "dropped {dropped}");
    assert!((speed(&state) - 450.0).abs() < 0.01, "{state:?}");
}

#[test]
fn wall_runs_end_and_the_same_wall_cant_be_grabbed_again() {
    let world = side_wall();
    let mut ended_at = None;
    let mut ticks = 0;
    let state = run(&world, beside_wall(600.0), SPRINT, 4.0, |s| {
        ticks += 1;
        match (ended_at, s.wall) {
            (None, None) => ended_at = Some(ticks),
            (Some(_), Some(w)) => panic!("grabbed the wall again: {w:?}"),
            _ => {}
        }
    });
    let ended_at = ended_at.unwrap() as f32 * DT;
    assert!((ended_at - 1.5).abs() < 3.0 * DT, "ran for {ended_at} s");
    // Landing frees the wall for another run.
    assert!(state.on_ground && state.last_wall.is_none(), "{state:?}");
}

#[test]
fn no_wall_run_too_close_to_the_ground_or_too_slow() {
    let world = side_wall();
    let low = step(&beside_wall(10.0), &SPRINT, &tuning(), &world, DT);
    assert_eq!(low.wall, None, "{low:?}");

    let mut slow = beside_wall(300.0);
    slow.velocity.z = -100.0;
    let slow = step(&slow, &SPRINT, &tuning(), &world, DT);
    assert_eq!(slow.wall, None, "{slow:?}");
}

#[test]
fn wall_jump_pushes_off_and_up() {
    let world = side_wall();
    let t = tuning();
    let running = run(&world, beside_wall(300.0), SPRINT, 0.5, |_| {});
    assert!(running.wall.is_some());
    // Look 30° away from the wall (it's on the right) to kick off rather than hop.
    let jumped = step(
        &running,
        &MoveInput {
            jump: true,
            yaw: 30f32.to_radians(),
            ..SPRINT
        },
        &t,
        &world,
        DT,
    );
    assert_eq!(jumped.wall, None);
    assert_eq!(jumped.wall_jumps, 2);
    assert!(jumped.velocity.x < -250.0, "{jumped:?}");
    assert!(jumped.velocity.y > 200.0, "{jumped:?}");
    assert!(jumped.velocity.z < -400.0, "kept the run speed: {jumped:?}");
}

#[test]
fn wall_hang_holds_still_until_released_or_timed_out() {
    let world = side_wall();
    let t = tuning();
    let hang = MoveInput {
        hang: true,
        ..Default::default()
    };
    let mut start = beside_wall(300.0);
    start.velocity = Vec3::new(0.0, -200.0, 0.0);
    let grabbed = step(&start, &hang, &t, &world, DT);
    assert_eq!(grabbed.wall.map(|w| w.kind), Some(WallKind::Hang));

    let held = run(&world, grabbed, hang, 1.5, |s| {
        assert_eq!(s.velocity, Vec3::ZERO, "{s:?}");
        assert!((s.position.y - grabbed.position.y).abs() < 0.01, "{s:?}");
    });
    let released = run(&world, held, MoveInput::default(), 0.2, |_| {});
    assert!(released.wall.is_none() && released.velocity.y < 0.0);

    // Holding on past the time limit drops you, and you can't re-grab.
    let mut dropped_after = None;
    let mut ticks = 0;
    run(&world, grabbed, hang, 3.0, |s| {
        ticks += 1;
        if s.wall.is_none() {
            dropped_after.get_or_insert(ticks);
        } else {
            assert!(dropped_after.is_none(), "re-grabbed: {s:?}");
        }
    });
    let dropped_after = dropped_after.unwrap() as f32 * DT;
    assert!(
        (dropped_after - 2.0).abs() < 3.0 * DT,
        "hung for {dropped_after} s"
    );
}

#[test]
fn wall_jump_across_a_corridor_starts_a_run_on_the_far_wall() {
    // `side_wall` plus a second wall facing it, 300 units away (like the sandbox's).
    let mut world = side_wall();
    add_block(
        &mut world,
        Vec3::new(-220.0, 0.0, -5000.0),
        Vec3::new(-200.0, 1000.0, 1000.0),
    );
    let t = tuning();
    let running = run(&world, beside_wall(300.0), SPRINT, 0.3, |_| {});
    // Look 30° towards the far wall, as a player would. Holding forward while
    // looking straight down the corridor steers the push off away.
    let towards = MoveInput {
        yaw: 30f32.to_radians(),
        ..SPRINT
    };
    let mut state = step(
        &running,
        &MoveInput {
            jump: true,
            ..towards
        },
        &t,
        &world,
        DT,
    );
    let mut far_wall = None;
    for _ in 0..90 {
        state = step(&state, &towards, &t, &world, DT);
        if let Some(wall) = state.wall {
            far_wall = Some(wall);
            break;
        }
    }
    let wall = far_wall.unwrap_or_else(|| panic!("never reached the far wall: {state:?}"));
    assert_eq!(wall.kind, WallKind::Run);
    assert!(wall.plane.normal.abs_diff_eq(Vec3::X, 1e-3), "{wall:?}");
}

// ---- double jump ----

/// Jump from the floor, press jump again `delay` seconds later. Returns the height
/// gained after the second press, and the state just after it.
fn double_jump_gain(delay: f32) -> (f32, MovementState) {
    let world = floor();
    let t = tuning();
    let jump = MoveInput {
        jump: true,
        ..Default::default()
    };
    let mut state = step(&settle(&world, Vec3::ZERO), &jump, &t, &world, DT);
    state = run(&world, state, MoveInput::default(), delay, |_| {});
    let pressed_at = state.position.y;
    state = step(&state, &jump, &t, &world, DT);
    let after = state;
    let mut apex = state.position.y;
    run(&world, state, MoveInput::default(), 1.5, |s| {
        apex = apex.max(s.position.y);
    });
    (apex - pressed_at, after)
}

#[test]
fn double_jump_adds_its_height_wherever_you_press_it() {
    // The air jump replaces whatever vertical speed we had, so it adds the same
    // 40 units whether pressed rising, at the apex, or falling.
    for delay in [0.1, 0.35, 0.6] {
        let (gain, after) = double_jump_gain(delay);
        assert!(
            (gain - 40.0).abs() < 0.5,
            "pressed at {delay} s: gained {gain}"
        );
        assert_eq!(after.air_jumps, 0);
    }
}

#[test]
fn only_one_double_jump_until_you_land() {
    let world = floor();
    let t = tuning();
    let (_, after) = double_jump_gain(0.1);
    let jump = MoveInput {
        jump: true,
        ..Default::default()
    };
    let third = step(&after, &MoveInput::default(), &t, &world, DT);
    let third = step(&third, &jump, &t, &world, DT);
    assert!(
        third.velocity.y < after.velocity.y,
        "jumped a third time: {third:?}"
    );

    let landed = run(&world, third, MoveInput::default(), 2.0, |_| {});
    assert!(landed.on_ground && landed.air_jumps == 1, "{landed:?}");
}

#[test]
fn double_jump_turns_you_towards_the_stick() {
    let world = floor();
    let t = tuning();
    let mut state = MovementState::new(Vec3::new(0.0, 300.0, 0.0));
    state.velocity = Vec3::new(0.0, 0.0, -300.0);
    state.air_jumps = 1;
    let right = MoveInput {
        wish: Vec2::new(1.0, 0.0),
        jump: true,
        ..Default::default()
    };
    let turned = step(&state, &right, &t, &world, DT);
    assert!(
        turned.velocity.x > 290.0 && turned.velocity.z.abs() < 10.0,
        "{turned:?}"
    );
}

#[test]
fn grabbing_a_wall_gives_the_double_jump_back() {
    let world = side_wall();
    let mut state = beside_wall(300.0);
    state.air_jumps = 0;
    let state = step(&state, &SPRINT, &tuning(), &world, DT);
    assert!(state.wall.is_some() && state.air_jumps == 1, "{state:?}");
}

// ---- wall jump chains ----

#[test]
fn hops_chain_three_runs_into_one_long_one() {
    let world = side_wall();
    let t = tuning();
    let hop = MoveInput {
        jump: true,
        ..SPRINT
    };
    // Each run alone lasts 1.5 s. Hop once a second, three times.
    let mut state = beside_wall(300.0);
    for left in [2, 1, 0] {
        state = run(&world, state, SPRINT, 1.0, |s| {
            assert_eq!(s.wall.map(|w| w.kind), Some(WallKind::Run), "{s:?}");
        });
        state = step(&state, &hop, &t, &world, DT);
        let wall = state.wall.expect("a hop stays on the wall");
        assert_eq!(state.wall_jumps, left, "{state:?}");
        assert!(state.velocity.y > 0.0 && wall.time < DT, "{state:?}");
    }
    // The last run still gets its full 1.5 s: 4.5 s on one wall in all.
    state = run(&world, state, SPRINT, 1.4, |s| {
        assert!(s.wall.is_some(), "{s:?}");
    });
    // Landing refills the chain.
    let landed = run(&world, state, MoveInput::default(), 3.0, |_| {});
    assert!(landed.on_ground && landed.wall_jumps == 3, "{landed:?}");
}

#[test]
fn out_of_wall_jumps_a_jump_lets_go_and_double_jumps() {
    let world = side_wall();
    let t = tuning();
    let mut state = beside_wall(300.0);
    state.wall_jumps = 0;
    let running = run(&world, state, SPRINT, 0.2, |_| {});
    assert!(running.wall.is_some());
    let jumped = step(
        &running,
        &MoveInput {
            jump: true,
            ..SPRINT
        },
        &t,
        &world,
        DT,
    );
    assert_eq!(jumped.wall, None);
    assert_eq!(jumped.air_jumps, 0, "used the double jump: {jumped:?}");
    assert!(jumped.velocity.y > 0.0, "{jumped:?}");
    // And that wall can't be grabbed again before landing.
    run(&world, jumped, SPRINT, 1.0, |s| {
        assert_eq!(s.wall, None, "{s:?}")
    });
}

// ---- sliding ----

const SLIDE: MoveInput = MoveInput {
    sprint: true,
    slide: true,
    ..FORWARD
};

fn sprinting(world: &StaticWorld) -> MovementState {
    let state = run(world, settle(world, Vec3::ZERO), SPRINT, 1.0, |_| {});
    assert!((speed(&state) - 300.0).abs() < 0.01, "{state:?}");
    state
}

#[test]
fn sliding_from_a_sprint_boosts_then_glides() {
    let world = floor();
    let start = sprinting(&world);
    let boosted = step(&start, &SLIDE, &tuning(), &world, DT);
    assert!(boosted.sliding, "{boosted:?}");
    // 300 + 100 boost, minus one tick of slide friction.
    assert!(
        (speed(&boosted) - (400.0 - 100.0 * DT)).abs() < 0.01,
        "{boosted:?}"
    );
    // A second of slide friction (100 units/s²) takes 100 off.
    let glided = run(&world, boosted, SLIDE, 1.0, |s| assert!(s.sliding, "{s:?}"));
    assert!(
        (speed(&glided) - (300.0 - 100.0 * DT)).abs() < 0.5,
        "{glided:?}"
    );
    // Letting go ends it.
    let stood = step(&glided, &SPRINT, &tuning(), &world, DT);
    assert!(!stood.sliding, "{stood:?}");
}

#[test]
fn cannot_slide_from_a_walk() {
    let world = floor();
    let walking = run(&world, settle(&world, Vec3::ZERO), FORWARD, 1.0, |_| {});
    let tried = step(
        &walking,
        &MoveInput {
            slide: true,
            ..FORWARD
        },
        &tuning(),
        &world,
        DT,
    );
    assert!(!tried.sliding, "{tried:?}");
    assert!((speed(&tried) - 200.0).abs() < 0.01, "{tried:?}");
}

#[test]
fn slide_boost_has_a_cooldown() {
    let world = floor();
    let t = tuning();
    let slid = run(&world, sprinting(&world), SLIDE, 0.1, |_| {});
    let stood = step(&slid, &SPRINT, &t, &world, DT);
    let again = step(&stood, &SLIDE, &t, &world, DT);
    assert!(again.sliding, "{again:?}");
    assert!(speed(&again) < speed(&stood), "boosted again: {again:?}");
}

#[test]
fn slide_hopping_builds_speed_up_to_the_cap() {
    let world = floor();
    // Hold slide and keep pressing jump: every landing slides (boost) and jumps.
    let hop = MoveInput {
        jump: true,
        ..SLIDE
    };
    let mut fastest = 0.0f32;
    let state = run(&world, sprinting(&world), hop, 15.0, |s| {
        fastest = fastest.max(speed(s));
    });
    assert!(fastest <= 1000.0 + 1e-3, "went past the cap: {fastest}");
    assert!(speed(&state) > 990.0, "{state:?}");
}

#[test]
fn sliding_downhill_speeds_you_up() {
    let mut world = floor();
    add_ramp(&mut world, -100.0, 400.0, 30.0);
    // On the 30° ramp (its surface is 200·tan30° ≈ 115.5 up at z = -300), already
    // heading down it at 300 flat, holding slide.
    let tan = 30f32.to_radians().tan();
    let mut state = MovementState::new(Vec3::new(0.0, 200.0 * tan + 0.5, -300.0));
    state.velocity = Vec3::new(0.0, -300.0 * tan, 300.0);
    let slide = MoveInput {
        slide: true,
        ..Default::default()
    };
    let mut landed = None;
    let state = run(&world, state, slide, 0.3, |s| {
        if s.sliding && landed.is_none() {
            landed = Some(speed(s));
        }
    });
    let landed = landed.expect("never started sliding");
    assert!(state.sliding, "{state:?}");
    // Downhill pull is g·sin30°·cos30° ≈ 346 units/s², well over the friction.
    assert!(speed(&state) > landed + 40.0, "{landed} -> {state:?}");
}
