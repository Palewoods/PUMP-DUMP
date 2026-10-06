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

mod abilities;
mod character;
mod dev;
mod enemies;
mod health;
mod heart;
mod input;
mod map;
mod player;
mod retro;
mod rockets;
mod run;
mod sfx;
mod targets;
mod tuning;
mod viewmodel;
mod weapon;

use bevy::prelude::*;

/// Simulation ticks per second.
const TICK_HZ: f64 = 60.0;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "PUMP&DUMP".into(),
                        ..default()
                    }),
                    ..default()
                })
                // Blocky, unfiltered textures everywhere, for the retro look.
                .set(ImagePlugin::default_nearest()),
        )
        .insert_resource(Time::<Fixed>::from_hz(TICK_HZ))
        .add_plugins((
            // Before the player: its cameras draw into the retro screen.
            retro::RetroPlugin,
            tuning::TuningPlugin,
            input::InputPlugin,
            sfx::SfxPlugin,
            character::CharacterPlugin,
            map::MapPlugin,
            player::PlayerPlugin,
            targets::TargetsPlugin,
            weapon::WeaponPlugin,
            viewmodel::ViewModelPlugin,
            rockets::RocketsPlugin,
            abilities::AbilitiesPlugin,
            health::HealthPlugin,
            enemies::EnemiesPlugin,
        ))
        // Rust note: one `add_plugins` call takes at most 15 plugins at a time.
        .add_plugins((heart::HeartPlugin, run::RunPlugin, dev::DevPlugin))
        .run();
}
