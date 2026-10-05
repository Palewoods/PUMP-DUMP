//! Development helpers.
//!
//! `PUMPDUMP_SCREENSHOT=path.png cargo run -p pumpdump` saves a screenshot
//! after two seconds and quits: a quick check that the app starts and renders.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

pub struct DevPlugin;

impl Plugin for DevPlugin {
    fn build(&self, app: &mut App) {
        if let Some(path) = std::env::var_os("PUMPDUMP_SCREENSHOT") {
            app.insert_resource(ScreenshotTo(path.into()))
                .add_systems(Update, screenshot_then_exit);
        }
    }
}

#[derive(Resource)]
struct ScreenshotTo(PathBuf);

// Bevy note: `Local` is per-system state that survives between runs, like a
// `static` that belongs to this one function.
fn screenshot_then_exit(
    mut commands: Commands,
    time: Res<Time>,
    to: Res<ScreenshotTo>,
    mut taken: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    let t = time.elapsed_secs();
    if t > 2.0 && !*taken {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(to.0.clone()));
        *taken = true;
    }
    if t > 3.0 {
        exit.write(AppExit::Success);
    }
}
