# PUMP&DUMP

A fast-paced first-person movement shooter with a crunchy retro look, built in Rust with [Bevy](https://bevyengine.org/). You play a zombie gunslinger in a trenchcoat and fedora, hunted by humans with rifles. Movement is always fast: dash, slide, slam, wall jump, grapple and rocket jump your way around while you take them apart with a machete, a revolver, a double-barrel shotgun, a tommy gun and a rocket launcher.

Being dead, your heart doesn't beat on its own: you pull it out of your chest and squeeze it. Pick a faster heart rate at the start of a run and you'll be squeezing more often, but you'll hit harder, move faster and take more punishment.

Right now it's one test map with a squad of hunters and a few practice dummies.

## Your heart

At the start of every run you pick a heart rate. Your heart holds a measure of blood that drains away; press **Q** to pull it out (your weapons go away) and **fire** to squeeze it: each squeeze is one beat, a quarter of a full heart. Press Q again to get your weapon back. Once it's below half full, your heart rate's boosts start to fade (all but health), down to nothing on an empty heart; squeezing brings them straight back. Let it run dry and you **flatline**: the screen goes dark and you lose 10 health a second, with no healing, until you beat it again.

| Heart rate | BPM | A full heart lasts | Damage | Accuracy | Health | Speed | Reload |
|---|---|---|---|---|---|---|---|
| 1 Dormant | 30 | 40 s | x1.0 | normal | 100 | x1.0 | normal |
| 2 Steady | 60 | 25 s | x1.15 | +10% | 125 | x1.05 | 10% faster |
| 3 Quick | 90 | 15 s | x1.3 | +20% | 150 | x1.1 | 20% faster |
| 4 Pounding | 130 | 10 s | x1.5 | +30% | 175 | x1.15 | 30% faster |
| 5 Racing | 180 | 6 s | x1.75 | +40% | 200 | x1.2 | 40% faster |

At **Racing** you also pick a perk:

- **Bloodlust:** every kill heals you.
- **Pulse:** squeezing your heart when it needs it blasts everything close around you (at most every 2 seconds).
- **Second Heart:** the first time you die, you get back up.

Dying ends the run: back to the heart rate screen.

## Movement

- **Always fast.** No sprint: running is already 600 units/s (about 15 m/s), with instant starts and hard stops (no ice-skating). A little forgiveness too: you can still jump just after running off a ledge, and a jump pressed just before landing still fires.
- **Dash.** A burst of speed wherever you're steering, with gravity off and the view punching wide. You're untouchable while it lasts: enemy shots pass straight through you. Three charges that refill over time, and every kill hands one back. **Dash jump:** jump during a ground dash for a long, fast leap.
- **Slide.** Hold slide on the ground, from a standstill or at speed. Slides never slow down on the flat, speed up downhill, and get a kick each time you start one. Jump out and land still sliding to build speed (slide-hopping).
- **Ground slam.** Press slide in the air to drive straight down. Jump right as you land to **slam-bounce**: the further you fell, the higher you go.
- **Grappling hook.** Hold to throw it at any surface (up to 5000 units away) and get yanked towards it. Let go or jump to drop it and keep your momentum. Floating platforms around the map are there to hook onto.
- **Walls.** Touch any wall in the air and jump to wall jump, as many times as you like. Jump at a wall while moving along it to wall-run for up to 12 seconds; jump looking along it to *hop* and keep running, or looking away to *kick off*.
- **Rocket jump.** Rockets shove you without hurting you: fire one at your feet and jump.
- **Double jump**, which also turns you towards the direction you're holding, and **wall-hang** to grab a wall and hold still.

## Weapons

| Slot | Weapon | |
|---|---|---|
| 1 | **Machete** | A wide slash at close range. **F** swings it whatever weapon is out. |
| 2 | **Revolver** | Six shots, dead accurate, two to drop a hunter. |
| 3 | **Shotgun** | Double barrel: one barrel per click, 12 pellets per shot. |
| 4 | **Tommy gun** | Hold the trigger. A 50-round drum. |
| 5 | **Rocket launcher** | Rockets you can watch fly; the blast hurts everything near it and throws you around. |

Guns reload by themselves when empty, or press reload to top up. Switching weapons drops a reload in progress.

## The zombie

You're a zombie in a long trenchcoat and a fedora pulled down low: under the brim there's nothing but two glowing eyes. In first person you see his rotting grey-green hands on your weapons; press **V** to see him from over the shoulder.

## Enemies

Human hunters in field jackets, with caps or helmets and rifles. They move with the same movement code you do (slower, and without the tricks), so steps, ramps and walls work the same for them.

- Once one sees you, it hunts you: closes in, keeps its distance and strafes, and goes looking where it last saw you.
- Its rifle muzzle glows brighter and brighter just before it fires a slow glowing slug: dodge it, or dash through it. Each hit takes 15 of your 100 health.
- Killed hunters burst apart and come back at their post 10 seconds later.
- You heal slowly after a few seconds without being hit. At zero health the run is over.

## Look

The world renders at 270 pixels tall and is scaled up with hard pixel edges, a reduced colour palette and ordered dithering, over gritty low-resolution textures, dim warm light and thick distance fog.

## Tuning

Every movement number lives in [`crates/pumpdump/assets/movement.ron`](crates/pumpdump/assets/movement.ron), every weapon number in [`crates/pumpdump/assets/weapons.ron`](crates/pumpdump/assets/weapons.ron), and the heart rates and perks in [`crates/pumpdump/assets/heart.ron`](crates/pumpdump/assets/heart.ron). Edit any of them while the game is running: they reload when you save.

## Controls

| Action | Keyboard and mouse | Controller |
|---|---|---|
| Move | WASD | Left stick |
| Look | Mouse | Right stick |
| Fire (with the heart out: squeeze it) | Left mouse | RT |
| Pull out / put away your heart | Q | D-pad down |
| Machete slash | F | R3 |
| Pick weapon | 1–5, mouse wheel | Y (next) |
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

The first build compiles Bevy and takes several minutes. Later builds are quick. On Windows you can also double-click `run.cmd`. To make a shareable Windows build like the one on the Releases page, run `package.cmd`: it writes the game folder and a zip to `dist\`.

## Project layout

- [`crates/pumpdump-movement`](crates/pumpdump-movement): all the movement rules. It doesn't depend on Bevy: each tick is a plain function of (state, input, world), so it runs the same in tests, in the game, and later on a server for multiplayer. The enemies move with it too.
- [`crates/pumpdump`](crates/pumpdump): the game itself. Bevy app, map, characters, weapons, enemies, first-person camera, keyboard/mouse and controller input.

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
