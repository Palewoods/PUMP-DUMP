# PUMP&DUMP

A fast-paced first-person movement shooter with a crunchy retro look, built in Rust with [Bevy](https://bevyengine.org/). Movement takes its cue from ULTRAKILL: always fast, with a dash, endless wall jumps, ground slams and a grappling hook. You play a zombie gunslinger in a trenchcoat and fedora, up against more of his kind. Right now it's one test map: enemies that hunt you and shoot back, a few practice dummies, and a double-barrel shotgun.

## Movement

- **Always fast.** There's no sprint: running is already 600 units/s (about 15 m/s), with instant starts and hard stops (no ice-skating). A little forgiveness too: you can still jump just after running off a ledge, and a jump pressed just before landing still fires.
- **Dash.** A short burst of speed wherever you're steering, with gravity off. It costs one of 3 charges, which refill over time. **Dash jump:** jump during a ground dash for a long, fast leap.
- **Slide.** Hold slide on the ground, from a standstill or at speed. Slides never slow down on the flat, speed up downhill, and get a kick each time you start one. Jump out and land still sliding to build speed (slide-hopping), up to a cap of 2600 units/s.
- **Ground slam.** Press slide in the air to drive straight down. Jump right as you land to **slam-bounce**: the further you fell, the higher you go.
- **Grappling hook.** Hold to throw it at any surface (up to 5000 units away) and get yanked towards it. Let go or jump to drop it and keep your momentum. Floating platforms around the map are there to hook onto.
- **Walls.** Touch any wall in the air and jump to wall jump, as many times as you like. Jump at a wall while moving along it to wall-run for up to 12 seconds; jump looking along it to *hop* and keep running, or looking away to *kick off*.
- **Double jump**, which also turns you towards the direction you're holding, and **wall-hang** to grab a wall and hold still.

## The zombie

You're a zombie in a long trenchcoat and a fedora pulled down low: under the brim there's nothing but two glowing eyes. In first person you see his rotting grey-green hands on the shotgun; press **V** to see him from over the shoulder.

## Enemies

Zombie gunmen dressed like you, but with red eyes. They move with the same movement code you do (slower, and without the tricks), so steps, ramps and walls work the same for them.

- Once one sees you, it hunts you: closes in, keeps its distance and strafes, and goes looking where it last saw you.
- Its eyes flare white-hot just before it fires a slow glowing slug: dodge it. Each hit takes 15 of your 100 health.
- A close shotgun blast bursts one apart; it comes back at its post 10 seconds later.
- You heal slowly after a few seconds without being hit. At zero health you're back at the spawn.

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
| Dash | Shift | RB |
| Slide (on the ground) / slam (in the air) | Ctrl or C | B |
| Grappling hook | E or back mouse button (hold) | LB (hold) |
| Wall-hang | Right mouse (hold) | LT (hold) |
| First / third person | V | D-pad up |
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
