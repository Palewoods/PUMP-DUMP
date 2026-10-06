# PUMP&DUMP

A fast-paced first-person movement shooter with a crunchy retro look, built in Rust with [Bevy](https://bevyengine.org/). Right now it's a sandbox: a test map for running, sliding, wall-running and blasting target dummies with a double-barrel shotgun, all at high speed.

## Movement

- **Sprint and jump**, with snappy starts and hard stops (no ice-skating), plus a little forgiveness: you can still jump just after running off a ledge, and a jump pressed just before landing still fires.
- **Double jump.** It also turns you towards the direction you're holding.
- **Wall-run.** Jump at a wall while moving along it. Runs last up to 3.5 seconds.
- **Wall jumps.** Up to 3 before you land again. Jump looking along the wall to *hop* and keep running, or looking away to *kick off* towards another wall.
- **Wall-hang.** Grab a wall mid-air and hold still.
- **Slide.** Sprint and slide for a speed boost. Jump out of the slide and land still holding slide to boost again: speed keeps building up to a cap of 1600 units/s (about 40 m/s, over 3× sprint speed). Sliding downhill speeds you up.

## Weapons

- **Double-barrel shotgun.** One barrel per click, 12 pellets per shot. After both barrels it breaks open and reloads by itself, or press reload to top up. A close-range shot drops a target dummy.

## Look

The world renders at 270 pixels tall and is scaled up with hard pixel edges, a reduced colour palette and ordered dithering, over gritty low-resolution textures, dim warm light and thick distance fog.

## Tuning

Every movement number lives in [`crates/pumpdump/assets/movement.ron`](crates/pumpdump/assets/movement.ron). The shotgun's numbers are in [`crates/pumpdump/assets/shotgun.weapon.ron`](crates/pumpdump/assets/shotgun.weapon.ron). Edit either while the game is running: they reload when you save.

## Controls

| Action | Keyboard and mouse | Controller |
|---|---|---|
| Move | WASD | Left stick |
| Look | Mouse | Right stick |
| Fire | Left mouse | RT |
| Reload | R | X |
| Jump / double jump / wall jump | Space | A |
| Sprint | Shift (hold) | L3 (click to toggle) |
| Slide | Ctrl or C (hold) | B (hold) |
| Wall-hang | Right mouse (hold) | LT (hold) |
| Back to spawn | Backspace | Back |
| Free the mouse | Esc (click the window to capture it again) | |

## Download

Grab the latest Windows build from the [Releases page](https://github.com/Palewoods/PUMP-DUMP/releases): unzip it and double-click `PUMP&DUMP.exe`. Keep the `assets` folder next to the exe.

## Building from source

You need [Rust](https://www.rust-lang.org/tools/install) (latest stable).

- **Windows:** Rust also needs the Visual Studio C++ Build Tools. The Rust installer offers to set them up.
- **Linux:** Bevy needs a few system libraries (ALSA and udev). See [Bevy's Linux setup guide](https://github.com/bevyengine/bevy/blob/main/docs/linux_dependencies.md).

Then, from the repository folder:

```
cargo run -p pumpdump
```

The first build compiles Bevy and takes several minutes. Later builds are quick. To make a shareable Windows build like the one on the Releases page, run `package.cmd`: it writes the game folder and a zip to `dist\`. On Windows you can also double-click `run.cmd`.

## Project layout

- [`crates/pumpdump-movement`](crates/pumpdump-movement): all the movement rules. It doesn't depend on Bevy: each tick is a plain function of (state, input, world), so it runs the same in tests, in the game, and later on a server for multiplayer.
- [`crates/pumpdump`](crates/pumpdump): the game itself. Bevy app, greybox map, first-person camera, keyboard/mouse and controller input.

Run the tests with:

```
cargo test --workspace
```

## Contributing

Issues and pull requests are welcome. Before opening a pull request, please run:

```
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```

## License

PUMP&DUMP is free software, released under the [GNU General Public License v3.0](LICENSE). You can use, study, change and share it. If you distribute a game or program built on this code, you must release its source under the same license.
