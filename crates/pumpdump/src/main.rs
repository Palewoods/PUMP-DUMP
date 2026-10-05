// Release builds on Windows run as a normal app with no console window behind the
// game. Debug builds keep the console for log messages.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! PUMP&DUMP: a greybox map to run, jump and climb around in,
//! driven by `pumpdump-movement`.
//!
//! Run with `cargo run -p pumpdump`.
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
                title: "PUMP&DUMP".into(),
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
