//! Movement sandbox: a greybox map to run, jump and climb around in,
//! driven by `pf-movement`.
//!
//! Run with `cargo run -p pf-sandbox`.
//!
//! Bevy note: a Bevy app is a list of *plugins*, each adding *systems* (plain
//! functions) that run on *schedules* like `Startup`, `Update` and `FixedUpdate`.
//! Systems declare what data they need in their parameters, and Bevy hands it over.

mod dev;
mod input;
mod map;
mod player;
mod tuning;

use bevy::prelude::*;

/// Simulation ticks per second.
const TICK_HZ: f64 = 60.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Pilotframe movement sandbox".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(Time::<Fixed>::from_hz(TICK_HZ))
        .add_plugins((
            tuning::TuningPlugin,
            input::InputPlugin,
            map::MapPlugin,
            player::PlayerPlugin,
            dev::DevPlugin,
        ))
        .run();
}
