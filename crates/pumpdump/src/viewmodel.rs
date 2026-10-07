//! The first-person weapons (and the heart): models held in the zombie's rotting hands,
//! drawn by their own camera on their own render layer so they never poke into
//! walls. All boxes (the character kit's unit cube, stretched), to suit the
//! pixel look.
//!
//! Animated here: recoil, reloads (each gun its own: the shotgun breaks open,
//! the revolver's cylinder swings out and spins, the tommy gun swaps its drum,
//! the launcher takes a rocket at the back), raising a weapon after a switch,
//! the machete's swing, walking bob, and the muzzle flash. Upgraded weapons
//! get upgraded models (see `upgrades.rs`): a four-barrel launcher, say.
//!
//! The heart in your hand beats lub-dub in time with the heartbeat you hear:
//! the top chambers, then the bulk of it clenching and twisting, then the big
//! vessels swelling as the blood goes out. A squeeze clenches the fingers and
//! spurts blood. It darkens as it empties, and flatlined it hangs limp,
//! twitching now and then.

use std::f32::consts::PI;

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::character::{CharacterKit, SKIN, Style, limb, slab};
use crate::heart::{Boosts, Heart, Run};
use crate::player::{PlayerCamera, PlayerStatus, ThirdPerson};
use crate::retro::{RetroScreen, VIEW_MODEL_LAYER};
use crate::upgrades::Upgrades;
use crate::weapon::{
    Arsenal, GunFx, SWING_TIME, SWITCH_TIME, WeaponKind, WeaponsAsset, WeaponsHandle,
};

pub struct ViewModelPlugin;

impl Plugin for ViewModelPlugin {
    fn build(&self, app: &mut App) {
        // After Startup, so the player's camera exists to attach the weapons to.
        app.add_systems(PostStartup, spawn_view_models)
            .add_systems(Update, (animate, animate_heart, drip));
    }
}

/// Field of view the weapons are drawn with, degrees. Fixed, so they don't
/// stretch when the world's FOV widens at speed.
const VIEW_MODEL_FOV: f32 = 60.0;
/// Where the weapons sit relative to the eye, units: right, down, forwards (-Z).
const REST: Vec3 = Vec3::new(7.5, -8.0, -14.0);
/// How fast recoil settles, per second.
const KICK_RECOVERY: f32 = 9.0;
/// Brightness of the muzzle flash's light on the surroundings, lumens.
const MUZZLE_LIGHT: f32 = 4.0e9;

/// The weapons, moved around by recoil, reloads and walking.
#[derive(Component)]
struct ViewModel;

/// One weapon's model: shown while that weapon is out. Its look: 0 as it
/// comes, 1 upgraded (for weapons whose first upgrade changes how they look).
#[derive(Component)]
struct WeaponModel(WeaponKind, u8);

/// Which look a weapon has with these upgrades.
fn look_for(kind: WeaponKind, upgrades: &Upgrades) -> u8 {
    u8::from(kind != WeaponKind::Revolver && upgrades.has(kind, 1))
}

#[derive(Component)]
struct MuzzleFlash;

/// The heart in your hand, shown instead of a weapon while you hold it.
#[derive(Component)]
struct HeartModel;

/// The moving parts of the heart in your hand.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum HeartPart {
    /// The whole heart (not the hand): sways, twitches, sags.
    Muscle,
    /// The bulk of it: clenches (and twists) on the second beat.
    Ventricles,
    /// The two lobes on top: squeeze first.
    Atria,
    /// The big vessels: swell as the blood goes out.
    Vessels,
    /// The fingers round its front: close on a squeeze.
    Fingers,
}

/// Where each part of the heart squeezes towards, in the heart's own space.
const VENTRICLES_AT: Vec3 = Vec3::new(0.0, 2.0, 0.0);
const ATRIA_AT: Vec3 = Vec3::new(0.0, 3.3, 0.5);
const VESSELS_AT: Vec3 = Vec3::new(0.4, 3.4, 0.0);

/// A drop of blood spurting out of the heart on a squeeze.
#[derive(Component)]
struct HeartDrop {
    velocity: Vec3,
    life: f32,
}

/// The heart's colours (changed as it empties) and what drops are made of.
#[derive(Resource)]
struct HeartLook {
    muscle: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    drop: Handle<StandardMaterial>,
}

/// Where the heart is held, relative to the weapons' resting place: low, left
/// of the middle, out of the way of the crosshair. And how big it's drawn.
const HEART_AT: Vec3 = Vec3::new(-10.0, 3.5, 0.0);
const HEART_SCALE: f32 = 0.55;

/// A part of a weapon that moves by itself when reloading.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Piece {
    /// The weapon's body and the hands on it: it only moves as a whole.
    Frame,
    /// Shotgun: the barrels, with the left hand on the fore-end, hinging down.
    Barrels,
    /// Shotgun: two fresh shells, slid into the open breech.
    Shells,
    /// Shotgun: the left hand, off the fore-end to fetch the shells.
    ShellHand,
    /// Revolver: the cylinder, swung out to the side and spun.
    Cylinder,
    /// Tommy gun: the drum, dropped out and replaced.
    Drum,
    /// Tommy gun: the bolt's knob, pulled back to cock it.
    Bolt,
    /// Rocket launcher: the next rocket, pushed in at the back.
    Rocket,
    /// Rocket launcher: the left hand, off the grip to fetch the rocket.
    Hand,
}

impl Piece {
    const ALL: [Piece; 9] = [
        Piece::Frame,
        Piece::Barrels,
        Piece::Shells,
        Piece::ShellHand,
        Piece::Cylinder,
        Piece::Drum,
        Piece::Bolt,
        Piece::Rocket,
        Piece::Hand,
    ];
}

/// The pivot of one weapon's moving piece; the piece's boxes hang off it.
#[derive(Component)]
struct ReloadPiece(WeaponKind, Piece);

/// Where the pieces turn or slide from, in their weapon's space.
const SHOTGUN_HINGE: Vec3 = Vec3::new(0.0, -1.0, -0.6);
const SHOTGUN_HAND: Vec3 = Vec3::new(0.0, -2.7, -9.5);
const CYLINDER: Vec3 = Vec3::new(0.0, 0.1, -0.8);
/// The arm the revolver's cylinder swings out on.
const CRANE: Vec3 = Vec3::new(-1.2, -1.1, -0.8);
const DRUM: Vec3 = Vec3::new(0.0, -3.6, -2.0);
const BOLT: Vec3 = Vec3::new(0.0, 1.65, -2.0);
const ROCKET: Vec3 = Vec3::new(0.0, 1.5, 5.0);
const LAUNCHER_HAND: Vec3 = Vec3::new(0.0, -2.8, -8.0);

fn pivot(piece: Piece) -> Vec3 {
    match piece {
        Piece::Frame => Vec3::ZERO,
        Piece::Barrels | Piece::Shells => SHOTGUN_HINGE,
        Piece::ShellHand => SHOTGUN_HAND,
        Piece::Cylinder => CYLINDER,
        Piece::Drum => DRUM,
        Piece::Bolt => BOLT,
        Piece::Rocket => ROCKET,
        Piece::Hand => LAUNCHER_HAND,
    }
}

/// Lights up the surroundings for a moment on each shot.
#[derive(Component)]
struct FlashLight;

/// Where each model sits under the view model, how big it's drawn, and where
/// its muzzle is (in the model's own, unscaled space).
fn placement(kind: WeaponKind, look: u8) -> (Vec3, f32, Vec3) {
    match kind {
        // The quad launcher sits a little further forward, so you can see down
        // its four tubes.
        WeaponKind::Launcher if look == 1 => {
            (Vec3::new(3.5, 1.5, -4.0), 0.7, Vec3::new(0.0, 1.5, -23.0))
        }
        WeaponKind::Machete => (Vec3::new(2.0, 1.0, 1.0), 1.0, Vec3::new(0.0, 0.0, -10.0)),
        WeaponKind::Revolver => (Vec3::new(0.0, 0.5, 1.0), 1.0, Vec3::new(0.0, 0.6, -11.2)),
        WeaponKind::Shotgun => (Vec3::ZERO, 1.0, Vec3::new(0.0, 0.0, -22.0)),
        WeaponKind::TommyGun => (Vec3::ZERO, 1.0, Vec3::new(0.0, 0.4, -18.2)),
        // Big: drawn smaller and tucked further out to the right, on the shoulder.
        WeaponKind::Launcher => (Vec3::new(4.5, 1.0, 3.0), 0.7, Vec3::new(0.0, 1.5, -23.0)),
    }
}

/// Materials the models are made of.
struct Stuff {
    metal: Handle<StandardMaterial>,
    steel: Handle<StandardMaterial>,
    wood: Handle<StandardMaterial>,
    leather: Handle<StandardMaterial>,
    olive: Handle<StandardMaterial>,
    red: Handle<StandardMaterial>,
    brass: Handle<StandardMaterial>,
    skin: Handle<StandardMaterial>,
    rot: Handle<StandardMaterial>,
    coat: Handle<StandardMaterial>,
}

/// One box of a weapon model: which piece it moves with, what it's made of,
/// and where it is.
type Part = (Piece, Handle<StandardMaterial>, Transform);

/// The boxes making up one weapon and the hands holding it, in its own space
/// (pointing along -Z), as it comes (`look` 0) or upgraded (1).
fn parts(kind: WeaponKind, look: u8, m: &Stuff) -> Vec<Part> {
    let mut boxes = base_parts(kind, m);
    if look == 0 {
        return boxes;
    }
    let v = Vec3::new;
    let frame =
        |material: &Handle<StandardMaterial>, t: Transform| (Piece::Frame, material.clone(), t);
    match kind {
        // The cleaver: a broader, longer blade.
        WeaponKind::Machete => {
            boxes.retain(|(_, material, _)| *material != m.steel);
            boxes.push(frame(
                &m.steel,
                limb(v(0.0, -1.2, 1.5), v(0.0, 1.4, -17.0), Vec2::new(0.6, 5.4)),
            ));
        }
        WeaponKind::Revolver => {}
        // Quad barrel: another barrel either side, and shells for them.
        WeaponKind::Shotgun => {
            for x in [-3.0, 3.0] {
                boxes.push((
                    Piece::Barrels,
                    m.metal.clone(),
                    slab(v(x, 0.0, -10.5), v(1.6, 1.6, 21.0)),
                ));
                boxes.push((
                    Piece::Shells,
                    m.red.clone(),
                    slab(v(x, 0.0, -2.0), v(1.2, 1.2, 2.6)),
                ));
                boxes.push((
                    Piece::Shells,
                    m.brass.clone(),
                    slab(v(x, 0.0, -0.4), v(1.4, 1.4, 0.7)),
                ));
            }
        }
        // The big drum.
        WeaponKind::TommyGun => {
            boxes.retain(|(piece, _, _)| *piece != Piece::Drum);
            let middle = DRUM - Vec3::Y;
            boxes.push((Piece::Drum, m.metal.clone(), slab(middle, v(2.2, 8.6, 8.6))));
            boxes.push((Piece::Drum, m.steel.clone(), slab(middle, v(2.5, 2.6, 2.6))));
        }
        // Four tubes in a square, a red warhead peeking out of each.
        WeaponKind::Launcher => {
            boxes.retain(|(piece, material, t)| {
                !(*piece == Piece::Frame
                    && (*material == m.olive || *material == m.red || t.translation.z.abs() > 11.0))
            });
            for (x, y) in [(-2.1, -0.6), (2.1, -0.6), (-2.1, 3.6), (2.1, 3.6)] {
                boxes.push(frame(&m.olive, slab(v(x, y, -5.0), v(2.6, 2.6, 32.0))));
                boxes.push(frame(&m.red, slab(v(x, y, -21.6), v(1.4, 1.4, 1.4))));
                // A collar and a dark bore at the back of each tube: what you
                // see of it from behind.
                boxes.push(frame(&m.metal, slab(v(x, y, 10.6), v(3.0, 3.0, 1.2))));
                boxes.push(frame(&m.leather, slab(v(x, y, 11.25), v(1.6, 1.6, 0.1))));
            }
            // Bands holding the tubes together.
            for z in [-19.0, -5.0] {
                boxes.push(frame(&m.metal, slab(v(0.0, 1.5, z), v(7.6, 7.6, 0.8))));
            }
        }
    }
    boxes
}

fn base_parts(kind: WeaponKind, m: &Stuff) -> Vec<Part> {
    let v = Vec3::new;
    let tilt = |t: Transform, x: f32| t.with_rotation(Quat::from_rotation_x(x));
    let p = |material: &Handle<StandardMaterial>, t: Transform| (Piece::Frame, material.clone(), t);
    let q = |piece: Piece, material: &Handle<StandardMaterial>, t: Transform| {
        (piece, material.clone(), t)
    };
    match kind {
        WeaponKind::Machete => vec![
            p(&m.leather, slab(v(0.0, -2.0, 4.5), v(1.4, 1.8, 5.0))),
            p(&m.metal, slab(v(0.0, -1.8, 1.8), v(2.6, 2.2, 0.6))),
            p(
                &m.steel,
                limb(v(0.0, -1.4, 1.5), v(0.0, 0.6, -15.0), Vec2::new(0.4, 3.2)),
            ),
            p(&m.skin, slab(v(0.0, -2.4, 4.4), v(3.0, 3.4, 4.0))),
            p(&m.rot, slab(v(0.0, -0.6, 4.6), v(2.2, 0.4, 2.4))),
            p(
                &m.coat,
                limb(v(0.5, -3.8, 6.5), v(3.5, -10.0, 19.0), Vec2::splat(5.2)),
            ),
        ],
        WeaponKind::Revolver => vec![
            p(&m.metal, slab(v(0.0, 0.0, 0.0), v(1.8, 2.2, 4.5))),
            q(Piece::Cylinder, &m.metal, slab(CYLINDER, v(2.8, 2.8, 2.8))),
            q(
                Piece::Cylinder,
                &m.brass,
                slab(CYLINDER + v(0.75, 0.0, 1.35), v(0.5, 0.5, 0.3)),
            ),
            q(
                Piece::Cylinder,
                &m.brass,
                slab(CYLINDER + v(-0.75, 0.0, 1.35), v(0.5, 0.5, 0.3)),
            ),
            q(
                Piece::Cylinder,
                &m.brass,
                slab(CYLINDER + v(0.38, 0.65, 1.35), v(0.5, 0.5, 0.3)),
            ),
            q(
                Piece::Cylinder,
                &m.brass,
                slab(CYLINDER + v(-0.38, 0.65, 1.35), v(0.5, 0.5, 0.3)),
            ),
            q(
                Piece::Cylinder,
                &m.brass,
                slab(CYLINDER + v(0.38, -0.65, 1.35), v(0.5, 0.5, 0.3)),
            ),
            q(
                Piece::Cylinder,
                &m.brass,
                slab(CYLINDER + v(-0.38, -0.65, 1.35), v(0.5, 0.5, 0.3)),
            ),
            p(&m.metal, slab(v(0.0, 0.6, -6.5), v(1.1, 1.1, 9.0))),
            p(&m.metal, slab(v(0.0, 1.3, -10.6), v(0.3, 0.6, 0.6))),
            p(
                &m.metal,
                tilt(slab(v(0.0, 1.4, 2.4), v(0.6, 1.2, 1.0)), -0.5),
            ),
            p(
                &m.wood,
                tilt(slab(v(0.0, -2.6, 3.0), v(1.8, 4.2, 2.4)), 0.35),
            ),
            p(&m.metal, slab(v(0.0, -1.4, 0.8), v(0.5, 1.2, 2.0))),
            // Right hand round the grip, finger on the trigger; left cupping it.
            p(&m.skin, slab(v(0.0, -2.8, 3.2), v(3.0, 3.6, 3.4))),
            p(&m.skin, slab(v(0.4, -1.2, 0.6), v(0.8, 0.8, 2.2))),
            p(&m.rot, slab(v(0.0, -1.0, 3.6), v(2.2, 0.4, 2.0))),
            p(&m.skin, slab(v(-1.2, -4.0, 2.8), v(2.6, 2.2, 3.6))),
            p(
                &m.coat,
                limb(v(0.5, -4.5, 5.0), v(3.5, -10.0, 18.0), Vec2::splat(5.2)),
            ),
            p(
                &m.coat,
                limb(v(-1.5, -5.0, 4.0), v(-12.0, -9.0, 14.0), Vec2::splat(5.2)),
            ),
        ],
        WeaponKind::Shotgun => vec![
            q(
                Piece::Barrels,
                &m.metal,
                slab(v(-0.85, 0.0, -10.5), v(1.6, 1.6, 21.0)),
            ),
            q(
                Piece::Barrels,
                &m.metal,
                slab(v(0.85, 0.0, -10.5), v(1.6, 1.6, 21.0)),
            ),
            q(
                Piece::Barrels,
                &m.metal,
                slab(v(0.0, 0.75, -10.5), v(0.7, 0.4, 20.0)),
            ),
            p(&m.metal, slab(v(0.0, -0.5, 2.5), v(3.4, 2.8, 6.5))),
            p(
                &m.metal,
                tilt(slab(v(-0.9, 1.2, 4.8), v(0.6, 1.4, 0.9)), -0.4),
            ),
            p(
                &m.metal,
                tilt(slab(v(0.9, 1.2, 4.8), v(0.6, 1.4, 0.9)), -0.4),
            ),
            q(
                Piece::Barrels,
                &m.wood,
                slab(v(0.0, -1.35, -9.0), v(2.6, 1.5, 10.0)),
            ),
            p(
                &m.wood,
                tilt(slab(v(0.0, -2.9, 11.5), v(2.4, 3.4, 13.0)), -0.22),
            ),
            // Right hand round the wrist of the stock, a finger on the trigger;
            // left cupping the fore-end, thumb and claws either side.
            p(&m.skin, slab(v(0.3, -2.6, 7.0), v(3.4, 3.4, 4.0))),
            p(&m.skin, slab(v(0.2, -4.2, 6.0), v(3.0, 1.2, 2.4))),
            p(&m.skin, slab(v(0.5, -2.4, 4.0), v(0.9, 0.9, 2.4))),
            p(&m.rot, slab(v(0.3, -0.9, 7.2), v(2.4, 0.4, 2.4))),
            p(
                &m.coat,
                limb(v(0.6, -3.0, 8.5), v(3.5, -9.0, 22.0), Vec2::splat(5.2)),
            ),
            q(
                Piece::ShellHand,
                &m.skin,
                slab(v(0.0, -2.7, -9.5), v(3.6, 2.2, 5.0)),
            ),
            q(
                Piece::ShellHand,
                &m.skin,
                slab(v(-1.8, -1.3, -9.5), v(1.0, 1.4, 3.6)),
            ),
            q(
                Piece::ShellHand,
                &m.skin,
                slab(v(1.8, -1.4, -9.8), v(1.0, 1.6, 4.4)),
            ),
            q(
                Piece::ShellHand,
                &m.rot,
                slab(v(0.0, -3.9, -9.0), v(2.6, 0.4, 3.0)),
            ),
            q(
                Piece::ShellHand,
                &m.coat,
                limb(v(-0.5, -3.6, -7.5), v(-14.0, -8.0, 9.0), Vec2::splat(5.2)),
            ),
            // Two fresh shells, red with brass bases, sized to vanish inside
            // the barrels once they're in.
            q(
                Piece::Shells,
                &m.red,
                slab(v(-0.85, 0.0, -2.0), v(1.2, 1.2, 2.6)),
            ),
            q(
                Piece::Shells,
                &m.red,
                slab(v(0.85, 0.0, -2.0), v(1.2, 1.2, 2.6)),
            ),
            q(
                Piece::Shells,
                &m.brass,
                slab(v(-0.85, 0.0, -0.4), v(1.4, 1.4, 0.7)),
            ),
            q(
                Piece::Shells,
                &m.brass,
                slab(v(0.85, 0.0, -0.4), v(1.4, 1.4, 0.7)),
            ),
        ],
        WeaponKind::TommyGun => {
            let mut boxes = vec![
                p(&m.metal, slab(v(0.0, 0.0, 0.0), v(2.4, 2.6, 9.0))),
                p(&m.metal, slab(v(0.0, 0.4, -9.5), v(2.0, 2.0, 10.0))),
                p(&m.metal, slab(v(0.0, 0.4, -15.5), v(1.0, 1.0, 3.0))),
                p(&m.metal, slab(v(0.0, 0.4, -17.2), v(1.4, 1.4, 1.4))),
                // The drum magazine.
                q(Piece::Drum, &m.metal, slab(DRUM, v(1.8, 6.5, 6.5))),
                q(Piece::Drum, &m.steel, slab(DRUM, v(2.1, 2.0, 2.0))),
                q(Piece::Bolt, &m.steel, slab(BOLT, v(0.8, 0.7, 1.0))),
                p(
                    &m.wood,
                    tilt(slab(v(0.0, -3.0, -8.0), v(1.6, 3.8, 1.8)), 0.2),
                ),
                p(
                    &m.wood,
                    tilt(slab(v(0.0, -3.0, 3.2), v(1.6, 3.8, 2.0)), 0.3),
                ),
                p(
                    &m.wood,
                    limb(v(0.0, -0.6, 4.5), v(0.0, -2.4, 14.0), Vec2::new(2.2, 3.2)),
                ),
                p(&m.skin, slab(v(0.0, -3.2, 3.4), v(2.8, 3.2, 3.2))),
                p(&m.skin, slab(v(0.4, -1.6, 1.6), v(0.8, 0.8, 2.0))),
                p(&m.skin, slab(v(0.0, -4.4, -8.0), v(3.0, 3.0, 3.0))),
                p(&m.rot, slab(v(0.0, -5.9, -8.0), v(2.2, 0.4, 2.2))),
                p(
                    &m.coat,
                    limb(v(0.6, -4.2, 5.0), v(3.5, -10.0, 18.0), Vec2::splat(5.2)),
                ),
                p(
                    &m.coat,
                    limb(v(-0.5, -5.5, -7.0), v(-14.0, -9.0, 8.0), Vec2::splat(5.2)),
                ),
            ];
            // Cooling fins round the barrel.
            for z in [-6.0, -8.0, -10.0, -12.0] {
                boxes.push(p(&m.metal, slab(v(0.0, 0.4, z), v(2.6, 2.6, 0.4))));
            }
            boxes
        }
        WeaponKind::Launcher => vec![
            p(&m.olive, slab(v(0.0, 1.5, -5.0), v(5.6, 5.6, 32.0))),
            p(&m.metal, slab(v(0.0, 1.5, -21.5), v(6.4, 6.4, 1.4))),
            p(&m.red, slab(v(0.0, 1.5, -21.0), v(3.0, 3.0, 1.2))),
            p(&m.metal, slab(v(0.0, 1.5, 11.5), v(6.6, 6.6, 2.0))),
            p(&m.metal, slab(v(-3.6, 3.0, -4.0), v(1.0, 2.6, 3.0))),
            p(&m.metal, slab(v(0.0, -2.6, 1.0), v(1.6, 4.0, 2.0))),
            p(&m.metal, slab(v(0.0, -2.4, -8.0), v(1.6, 3.6, 1.8))),
            p(&m.skin, slab(v(0.0, -2.8, 1.2), v(2.8, 3.4, 3.2))),
            q(Piece::Hand, &m.skin, slab(LAUNCHER_HAND, v(3.0, 3.0, 3.0))),
            p(&m.rot, slab(v(0.0, -1.0, 1.4), v(2.0, 0.4, 2.2))),
            p(
                &m.coat,
                limb(v(0.5, -4.2, 3.0), v(3.5, -10.0, 16.0), Vec2::splat(5.2)),
            ),
            q(
                Piece::Hand,
                &m.coat,
                limb(v(-0.5, -4.4, -7.0), v(-14.0, -9.0, 8.0), Vec2::splat(5.2)),
            ),
            // The next rocket: nose, warhead, body and fins, and the hand
            // shoving it in.
            q(
                Piece::Rocket,
                &m.red,
                slab(v(0.0, 1.5, -2.6), v(1.4, 1.4, 1.2)),
            ),
            q(
                Piece::Rocket,
                &m.red,
                slab(v(0.0, 1.5, -0.5), v(2.4, 2.4, 3.0)),
            ),
            q(
                Piece::Rocket,
                &m.olive,
                slab(v(0.0, 1.5, 5.0), v(2.0, 2.0, 8.0)),
            ),
            q(
                Piece::Rocket,
                &m.metal,
                slab(v(0.0, 1.5, 8.5), v(4.4, 0.4, 2.0)),
            ),
            q(
                Piece::Rocket,
                &m.metal,
                slab(v(0.0, 1.5, 8.5), v(0.4, 4.4, 2.0)),
            ),
            q(
                Piece::Rocket,
                &m.skin,
                slab(v(0.0, 1.0, 11.0), v(3.0, 3.4, 3.0)),
            ),
            q(
                Piece::Rocket,
                &m.coat,
                limb(v(0.0, 0.0, 12.0), v(-8.0, -8.0, 26.0), Vec2::splat(5.2)),
            ),
        ],
    }
}

fn spawn_view_models(
    mut commands: Commands,
    eye: Single<Entity, With<PlayerCamera>>,
    screen: Res<RetroScreen>,
    kit: Res<CharacterKit>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let layer = RenderLayers::layer(VIEW_MODEL_LAYER);
    let mut matte = |color: Color, metallic: f32, roughness: f32| {
        materials.add(StandardMaterial {
            base_color: color,
            metallic,
            perceptual_roughness: roughness,
            reflectance: 0.1,
            ..default()
        })
    };
    let stuff = Stuff {
        metal: matte(Color::srgb(0.17, 0.17, 0.19), 0.7, 0.45),
        steel: matte(Color::srgb(0.62, 0.62, 0.6), 0.8, 0.35),
        wood: matte(Color::srgb(0.36, 0.19, 0.09), 0.0, 0.85),
        leather: matte(Color::srgb(0.12, 0.08, 0.05), 0.0, 0.9),
        olive: matte(Color::srgb(0.28, 0.31, 0.2), 0.2, 0.7),
        red: matte(Color::srgb(0.55, 0.1, 0.06), 0.0, 0.6),
        brass: matte(Color::srgb(0.7, 0.52, 0.2), 0.8, 0.4),
        skin: kit.skin(),
        rot: matte(SKIN.darker(0.15), 0.0, 1.0),
        coat: matte(Style::PLAYER.coat, 0.0, 1.0),
    };
    let fire = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.8, 0.35),
        unlit: true,
        ..default()
    });
    let flash_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let mut wet = |color: Color, roughness: f32| {
        materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: roughness,
            reflectance: 0.4,
            ..default()
        })
    };
    let heart_stuff = HeartStuff {
        muscle: wet(ALIVE, 0.35),
        dark: wet(ALIVE_DARK, 0.45),
        artery: wet(Color::srgb(0.55, 0.12, 0.2), 0.4),
        vein: wet(Color::srgb(0.2, 0.08, 0.25), 0.5),
    };
    commands.insert_resource(HeartLook {
        muscle: heart_stuff.muscle.clone(),
        dark: heart_stuff.dark.clone(),
        drop: materials.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.02, 0.04),
            unlit: true,
            ..default()
        }),
    });

    commands.entity(*eye).with_children(|eye| {
        // Draws only the weapons' layer, on top of the world camera's picture.
        eye.spawn((
            Camera3d::default(),
            Camera {
                order: 1,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            screen.target(),
            Msaa::Off,
            Projection::Perspective(PerspectiveProjection {
                fov: VIEW_MODEL_FOV.to_radians(),
                near: 0.5,
                far: 200.0,
                ..default()
            }),
            layer.clone(),
        ));
        eye.spawn((
            FlashLight,
            PointLight {
                intensity: 0.0,
                range: 1500.0,
                color: Color::srgb(1.0, 0.7, 0.35),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, -40.0),
            RenderLayers::from_layers(&[0, VIEW_MODEL_LAYER]),
        ));
        eye.spawn((
            ViewModel,
            Transform::from_translation(REST),
            Visibility::default(),
        ))
        .with_children(|view| {
            let looks = WeaponKind::ALL
                .into_iter()
                .flat_map(|kind| [(kind, 0), (kind, 1)])
                .filter(|&(kind, look)| look == 0 || kind != WeaponKind::Revolver);
            for (kind, look) in looks {
                let (offset, scale, _) = placement(kind, look);
                view.spawn((
                    WeaponModel(kind, look),
                    Transform::from_translation(offset).with_scale(Vec3::splat(scale)),
                    Visibility::Hidden,
                ))
                .with_children(|model| {
                    let parts = parts(kind, look, &stuff);
                    for piece in Piece::ALL {
                        let mut boxes = parts.iter().filter(|(p, _, _)| *p == piece).peekable();
                        if boxes.peek().is_none() {
                            continue;
                        }
                        // Each moving piece hangs off its own pivot, so it can
                        // turn and slide as one.
                        let at = pivot(piece);
                        let mut holder =
                            model.spawn((Transform::from_translation(at), Visibility::Inherited));
                        if piece != Piece::Frame {
                            holder.insert(ReloadPiece(kind, piece));
                        }
                        holder.with_children(|holder| {
                            for (_, material, transform) in boxes {
                                holder.spawn((
                                    Mesh3d(kit.cube()),
                                    MeshMaterial3d(material.clone()),
                                    transform.with_translation(transform.translation - at),
                                    layer.clone(),
                                    NotShadowCaster,
                                ));
                            }
                        });
                    }
                });
            }
            spawn_heart(view, &kit, &stuff, &heart_stuff, &layer);
            view.spawn((MuzzleFlash, Transform::default(), Visibility::Hidden))
                .with_children(|flash| {
                    // Two crossed slabs make a rough star.
                    for (size, angle) in [
                        (Vec3::new(5.0, 2.4, 1.5), 0.3),
                        (Vec3::new(2.4, 5.0, 1.5), -0.3),
                    ] {
                        flash.spawn((
                            Mesh3d(flash_mesh.clone()),
                            MeshMaterial3d(fire.clone()),
                            Transform::from_rotation(Quat::from_rotation_z(angle)).with_scale(size),
                            layer.clone(),
                            NotShadowCaster,
                        ));
                    }
                });
        });
    });
}

/// What the heart is made of.
struct HeartStuff {
    muscle: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    artery: Handle<StandardMaterial>,
    vein: Handle<StandardMaterial>,
}

/// The heart, cupped in the left hand with the fingers round its front.
fn spawn_heart(
    view: &mut ChildSpawnerCommands,
    kit: &CharacterKit,
    m: &Stuff,
    h: &HeartStuff,
    layer: &RenderLayers,
) {
    let v = Vec3::new;
    let part = |material: &Handle<StandardMaterial>, transform: Transform| {
        (
            Mesh3d(kit.cube()),
            MeshMaterial3d(material.clone()),
            transform,
            layer.clone(),
            NotShadowCaster,
        )
    };
    view.spawn((
        HeartModel,
        Transform::from_translation(HEART_AT).with_scale(Vec3::splat(HEART_SCALE)),
        Visibility::Hidden,
    ))
    .with_children(|held| {
        // Each moving part hangs off the point it squeezes towards, so its
        // boxes are placed relative to that.
        let at = |t: Transform, pivot: Vec3| t.with_translation(t.translation - pivot);
        held.spawn((
            HeartPart::Muscle,
            Transform::default(),
            Visibility::Inherited,
        ))
        .with_children(|heart| {
            // The bulk: the muscle, its pointed bottom, a vein across the front.
            heart
                .spawn((
                    HeartPart::Ventricles,
                    Transform::from_translation(VENTRICLES_AT),
                    Visibility::Inherited,
                ))
                .with_children(|bulk| {
                    let p = VENTRICLES_AT;
                    bulk.spawn(part(
                        &h.muscle,
                        at(slab(v(0.0, 0.0, 0.0), v(5.5, 6.5, 5.0)), p),
                    ));
                    bulk.spawn(part(
                        &h.muscle,
                        at(
                            slab(v(0.5, -3.6, 0.0), v(3.2, 2.2, 3.2))
                                .with_rotation(Quat::from_rotation_z(0.3)),
                            p,
                        ),
                    ));
                    bulk.spawn(part(
                        &h.vein,
                        at(slab(v(0.6, 0.6, -2.6), v(0.5, 4.5, 0.4)), p),
                    ));
                });
            // Two lobes on top.
            heart
                .spawn((
                    HeartPart::Atria,
                    Transform::from_translation(ATRIA_AT),
                    Visibility::Inherited,
                ))
                .with_children(|lobes| {
                    let p = ATRIA_AT;
                    lobes.spawn(part(
                        &h.dark,
                        at(slab(v(-1.7, 3.4, 0.4), v(2.8, 2.6, 3.0)), p),
                    ));
                    lobes.spawn(part(
                        &h.dark,
                        at(slab(v(1.6, 3.2, 0.7), v(2.6, 2.4, 2.8)), p),
                    ));
                });
            // The big vessels out of the top.
            heart
                .spawn((
                    HeartPart::Vessels,
                    Transform::from_translation(VESSELS_AT),
                    Visibility::Inherited,
                ))
                .with_children(|vessels| {
                    let p = VESSELS_AT;
                    vessels.spawn(part(
                        &h.artery,
                        at(
                            limb(v(0.6, 3.4, 0.0), v(1.0, 7.6, -0.6), Vec2::splat(1.7)),
                            p,
                        ),
                    ));
                    vessels.spawn(part(
                        &h.artery,
                        at(
                            limb(v(1.0, 7.2, -0.6), v(2.9, 8.3, 0.2), Vec2::splat(1.0)),
                            p,
                        ),
                    ));
                    vessels.spawn(part(
                        &h.vein,
                        at(
                            limb(v(-1.8, 3.6, 0.2), v(-2.8, 7.0, 1.2), Vec2::splat(1.3)),
                            p,
                        ),
                    ));
                });
        });
        // The hand: palm under, thumb up the side, sleeve back out of view.
        held.spawn(part(&m.skin, slab(v(0.0, -4.4, 0.5), v(6.2, 1.8, 5.6))));
        held.spawn(part(&m.skin, slab(v(-3.4, -0.5, 0.6), v(1.1, 4.0, 1.3))));
        held.spawn(part(&m.rot, slab(v(0.0, -5.4, 0.8), v(3.0, 0.4, 3.0))));
        held.spawn(part(
            &m.coat,
            limb(v(0.0, -5.4, 2.5), v(-6.0, -11.0, 16.0), Vec2::splat(5.4)),
        ));
        held.spawn((
            HeartPart::Fingers,
            Transform::default(),
            Visibility::Inherited,
        ))
        .with_children(|fingers| {
            for x in [-2.4, -0.8, 0.8, 2.4] {
                fingers.spawn(part(&m.skin, slab(v(x, -1.6, -2.9), v(1.0, 4.6, 1.0))));
                fingers.spawn(part(&m.rot, slab(v(x, 0.9, -2.7), v(0.8, 0.8, 0.8))));
            }
        });
    });
}

/// The heart's colours full of blood, and empty.
const ALIVE: Color = Color::srgb(0.55, 0.04, 0.06);
const ALIVE_DARK: Color = Color::srgb(0.34, 0.03, 0.05);
const DEAD: Color = Color::srgb(0.2, 0.08, 0.11);
const DEAD_DARK: Color = Color::srgb(0.12, 0.05, 0.08);
/// Where blood spurts from (the top of the big artery), in the held heart's
/// space, and how fast drops fall (units/s², in that space).
const SPURT_FROM: Vec3 = Vec3::new(1.0, 8.0, -0.6);
const DROP_GRAVITY: f32 = 90.0;
const DROP_LIFE: f32 = 0.5;

/// 0 -> 1 -> 0: up (eased) from `start` to `peak`, down again by `end`.
fn pulse(t: f32, start: f32, peak: f32, end: f32) -> f32 {
    if t <= start || t >= end {
        0.0
    } else if t < peak {
        ease((t - start) / (peak - start))
    } else {
        1.0 - ease((t - peak) / (end - peak))
    }
}

/// How hard each part of the heart is working `since` seconds into a beat
/// that lasts `period` seconds: (top chambers, the bulk, the vessels). The
/// lub (the top) comes first, then the dub (the bulk), then the blood goes out.
/// A real heart's squeeze takes about as long however fast it beats (only the
/// rest between beats shrinks); at the fastest rates it's hurried a little.
fn beat(since: f32, period: f32) -> (f32, f32, f32) {
    let k = (period / 0.8).min(1.0);
    (
        pulse(since, 0.0, 0.05 * k, 0.14 * k),
        pulse(since, 0.09 * k, 0.17 * k, 0.4 * k),
        pulse(since, 0.15 * k, 0.24 * k, 0.5 * k),
    )
}

/// Show the heart while it's held, and work it: see the module notes.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn animate_heart(
    mut commands: Commands,
    time: Res<Time>,
    heart: Res<Heart>,
    look: Res<HeartLook>,
    kit: Res<CharacterKit>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    model: Single<(Entity, &mut Visibility), With<HeartModel>>,
    mut parts: Query<(&HeartPart, &mut Transform)>,
    mut last_pump: Local<f32>,
) {
    let (model, mut shown) = model.into_inner();
    *shown = if heart.held {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    let flat = heart.flatlined();
    // Weaker beats as the blood runs out; none at all when it's gone.
    let strength = if flat {
        0.0
    } else {
        0.5 + 0.5 * (heart.blood / 0.5).min(1.0)
    };
    let (atria, bulk, vessels) = beat(heart.since_beat, 60.0 / heart.bpm.max(1.0));
    let (atria, bulk, vessels) = (atria * strength, bulk * strength, vessels * strength);
    // A squeeze: hard in, slower out.
    let squeeze = pulse(heart.since_squeeze, 0.0, 0.05, 0.3);
    // Flatlined, it sags, and every so often twitches.
    let now = time.elapsed_secs();
    let twitch_at = now % 1.7;
    let twitch = if flat && twitch_at < 0.4 {
        (now * 60.0).sin() * 0.05 * (1.0 - twitch_at / 0.4)
    } else {
        0.0
    };
    let sag = if flat { 1.0 } else { 0.0 };

    for (part, mut transform) in &mut parts {
        match part {
            HeartPart::Muscle => {
                transform.translation = Vec3::new(twitch * 4.0, 0.25 * bulk - 0.4 * sag, 0.0);
                transform.rotation = Quat::from_euler(EulerRot::XYZ, 0.15 * sag, 0.0, twitch);
                transform.scale = Vec3::new(1.0 + 0.03 * sag, 1.0 - 0.06 * sag, 1.0 + 0.03 * sag);
            }
            HeartPart::Ventricles => {
                let squash = 0.14 * bulk + 0.3 * squeeze;
                transform.scale = Vec3::new(1.0 - squash, 1.0 - 0.45 * squash, 1.0 - squash);
                // Hearts wring as they squeeze.
                transform.rotation = Quat::from_rotation_y(0.18 * bulk - 0.1 * squeeze);
            }
            HeartPart::Atria => {
                transform.scale = Vec3::splat(1.0 - 0.22 * atria - 0.2 * squeeze);
                transform.translation = ATRIA_AT - Vec3::Y * 0.3 * atria;
            }
            HeartPart::Vessels => {
                let swell = 0.45 * vessels + 0.35 * squeeze;
                transform.scale = Vec3::new(1.0 + swell, 1.0 + 0.1 * swell, 1.0 + swell);
            }
            HeartPart::Fingers => {
                transform.translation = Vec3::new(0.0, 0.0, 0.9 * squeeze + 0.12 * bulk);
                transform.rotation = Quat::from_rotation_x(-0.3 * squeeze);
            }
        }
    }

    // It darkens as it empties, and flushes for a moment with each squeeze.
    if heart.held {
        let full = heart.blood.sqrt();
        let flush = 0.15 * squeeze;
        let mix = |empty: Color, alive: Color| {
            let c = empty.mix(&alive, full).to_srgba();
            Color::srgb(c.red + flush, c.green, c.blue)
        };
        if let Some(mut material) = materials.get_mut(&look.muscle) {
            material.base_color = mix(DEAD, ALIVE);
        }
        if let Some(mut material) = materials.get_mut(&look.dark) {
            material.base_color = mix(DEAD_DARK, ALIVE_DARK);
        }
    }

    // A squeeze that put blood back spurts some out of the top.
    if heart.since_pump < *last_pump && heart.held {
        commands.entity(model).with_children(|held| {
            for k in 0..7 {
                let spread = k as f32 * 2.4; // golden angle
                let velocity = Vec3::new(
                    spread.cos() * 7.0,
                    20.0 + (k % 3) as f32 * 5.0,
                    spread.sin() * 5.0,
                );
                held.spawn((
                    HeartDrop {
                        velocity,
                        life: DROP_LIFE,
                    },
                    Mesh3d(kit.cube()),
                    MeshMaterial3d(look.drop.clone()),
                    Transform::from_translation(SPURT_FROM)
                        .with_scale(Vec3::splat(0.6 + 0.1 * (k % 3) as f32)),
                    RenderLayers::layer(VIEW_MODEL_LAYER),
                    NotShadowCaster,
                ));
            }
        });
    }
    *last_pump = heart.since_pump;
}

/// Spurted drops fly, fall, shrink and go.
fn drip(
    mut commands: Commands,
    time: Res<Time>,
    mut drops: Query<(Entity, &mut HeartDrop, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut drop, mut transform) in &mut drops {
        drop.life -= dt;
        if drop.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        drop.velocity.y -= DROP_GRAVITY * dt;
        transform.translation += drop.velocity * dt;
        transform.scale = Vec3::splat(0.7 * (drop.life / DROP_LIFE).sqrt());
    }
}

/// 0 -> 1 with a smooth start and end.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// ---- reloads ----
// Each is laid out over the reload's progress, 0 -> 1, timed to its sounds: the
// click at the halfway mark is the new rounds going in, the clack at the end is
// the gun closing up.

/// 0 -> 1, eased, over the stretch of the reload from `from` to `to`.
fn span(t: f32, from: f32, to: f32) -> f32 {
    ease((t - from) / (to - from))
}

/// Up over the start of the reload to `rise`, held, and down again from `fall`.
fn held(t: f32, rise: f32, fall: f32) -> f32 {
    span(t, 0.0, rise) * (1.0 - span(t, fall, 1.0))
}

/// A quick 0 -> 1 -> 0 between `from` and `to`.
fn bump(t: f32, from: f32, to: f32) -> f32 {
    let u = (t - from) / (to - from);
    if (0.0..1.0).contains(&u) {
        (u * PI).sin()
    } else {
        0.0
    }
}

/// How the whole weapon moves through its reload, `t` 0 -> 1: a shift, and a
/// turn (X, Y, Z angles). Nothing at either end.
fn reload_sway(kind: WeaponKind, t: f32) -> (Vec3, Vec3) {
    let v = Vec3::new;
    if t <= 0.0 || t >= 1.0 {
        return (Vec3::ZERO, Vec3::ZERO);
    }
    match kind {
        WeaponKind::Machete => (Vec3::ZERO, Vec3::ZERO),
        // Brought in and rolled over to look into the breech; a jolt as it
        // snaps shut.
        WeaponKind::Shotgun => {
            let h = held(t, 0.15, 0.85);
            let snap = bump(t, 0.9, 1.0);
            (
                v(-4.0, 5.5, -5.0) * h + v(0.0, 0.8, 0.0) * snap,
                v(-0.1, 0.45, -0.5) * h - v(0.15, 0.0, 0.0) * snap,
            )
        }
        // Tipped up and over to the left while the cylinder's out.
        WeaponKind::Revolver => {
            let h = held(t, 0.12, 0.88);
            (v(-5.0, 4.0, 0.0) * h, v(0.5, 0.3, -0.6) * h)
        }
        // Rolled to show the drum, then yanked back as the bolt is pulled.
        WeaponKind::TommyGun => {
            let h = held(t, 0.12, 0.9);
            let pull = bump(t, 0.8, 0.94);
            (
                v(-6.0, 5.0, -4.0) * h + v(0.0, 0.0, 2.5) * pull,
                v(0.1, 0.35, 0.3) * h + v(0.12, 0.0, 0.0) * pull,
            )
        }
        // Off the shoulder and tipped up, its back end down where you can
        // load it.
        WeaponKind::Launcher => {
            let h = held(t, 0.15, 0.78);
            (v(-9.0, 6.0, -12.0) * h, v(0.25, -0.35, 0.1) * h)
        }
    }
}

/// Where a moving piece is at reload progress `t` (in its weapon's space), and
/// whether it's shown. At rest at either end; the fresh shells and rocket are
/// only shown while going in.
fn piece_pose(piece: Piece, t: f32) -> (Transform, bool) {
    let v = Vec3::new;
    let at = pivot(piece);
    let rest = Transform::from_translation(at);
    let reloading = t > 0.0 && t < 1.0;
    match piece {
        Piece::Frame => (rest, true),
        // The barrels drop open on the hinge, the shells slide in along them,
        // and it snaps shut.
        Piece::Barrels | Piece::Shells => {
            let open = span(t, 0.0, 0.18) * (1.0 - span(t, 0.82, 0.95));
            let turn = Quat::from_rotation_x(-0.6 * open);
            if piece == Piece::Barrels {
                return (rest.with_rotation(turn), true);
            }
            // (Out of sight inside when not reloading.)
            let slide = if reloading {
                8.0 * (1.0 - span(t, 0.3, 0.55))
            } else {
                0.0
            };
            (
                Transform::from_translation(at + turn * v(0.0, 0.0, slide)).with_rotation(turn),
                t > 0.22 && reloading,
            )
        }
        // Out to the side on its crane, a spin, and flicked back in.
        Piece::Cylinder => {
            let out = span(t, 0.0, 0.15) * (1.0 - span(t, 0.85, 0.97));
            let swing = Quat::from_rotation_z(1.3 * out);
            let spin = Quat::from_rotation_z(4.0 * PI * span(t, 0.2, 0.75));
            (
                Transform::from_translation(CRANE + swing * (at - CRANE))
                    .with_rotation(swing * spin),
                true,
            )
        }
        // The empty drum slides out sideways and drops away; a full one
        // slides in.
        Piece::Drum => {
            if t < 0.32 {
                let out = span(t, 0.05, 0.18);
                let drop = span(t, 0.18, 0.3).powi(2);
                (
                    Transform::from_translation(
                        at + v(6.0, 0.0, 0.0) * out + v(2.0, -12.0, 0.0) * drop,
                    )
                    .with_rotation(Quat::from_rotation_z(-0.8 * drop)),
                    t < 0.3 || !reloading,
                )
            } else {
                let away = 1.0 - span(t, 0.34, 0.52);
                (
                    Transform::from_translation(at + v(7.0, -2.0, 0.0) * away),
                    t > 0.34 || !reloading,
                )
            }
        }
        // Pulled back and let go.
        Piece::Bolt => {
            let back = span(t, 0.8, 0.86) * (1.0 - span(t, 0.88, 0.93));
            (
                Transform::from_translation(at + v(0.0, 0.0, 3.0) * back),
                true,
            )
        }
        // Pushed in at the back of the tube.
        Piece::Rocket => {
            let slide = if reloading {
                24.0 * (1.0 - span(t, 0.2, 0.55))
            } else {
                0.0
            };
            (
                Transform::from_translation(at + v(0.0, 0.0, slide)),
                t > 0.15 && t < 0.6,
            )
        }
        // Off the gun (to fetch the rounds) and back.
        Piece::Hand | Piece::ShellHand => {
            let back = if piece == Piece::Hand {
                span(t, 0.62, 0.85)
            } else {
                span(t, 0.8, 0.95)
            };
            let away = span(t, 0.0, 0.15) * (1.0 - back);
            (
                Transform::from_translation(at + v(-3.0, -16.0, 6.0) * away),
                true,
            )
        }
    }
}

// Bevy note: the `Without` filters tell Bevy these queries never touch the same
// entity, which it needs to hand out the `&mut` components safely.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn animate(
    time: Res<Time>,
    arsenal: Res<Arsenal>,
    handle: Res<WeaponsHandle>,
    tunings: Res<Assets<WeaponsAsset>>,
    status: Res<PlayerStatus>,
    third_person: Res<ThirdPerson>,
    heart: Res<Heart>,
    (boosts, run, upgrades): (Res<Boosts>, Res<Run>, Res<Upgrades>),
    mut fx: ResMut<GunFx>,
    mut bob_phase: Local<f32>,
    view: Single<
        (&mut Transform, &mut Visibility),
        (
            With<ViewModel>,
            Without<WeaponModel>,
            Without<MuzzleFlash>,
            Without<ReloadPiece>,
        ),
    >,
    mut models: Query<
        (&WeaponModel, &mut Transform, &mut Visibility),
        (
            Without<ViewModel>,
            Without<MuzzleFlash>,
            Without<ReloadPiece>,
        ),
    >,
    mut pieces: Query<
        (&ReloadPiece, &mut Transform, &mut Visibility),
        (
            Without<ViewModel>,
            Without<WeaponModel>,
            Without<MuzzleFlash>,
        ),
    >,
    flash: Single<
        (&mut Transform, &mut Visibility),
        (
            With<MuzzleFlash>,
            Without<ViewModel>,
            Without<WeaponModel>,
            Without<ReloadPiece>,
        ),
    >,
    light: Single<&mut PointLight, With<FlashLight>>,
) {
    let dt = time.delta_secs();
    fx.kick *= (-KICK_RECOVERY * dt).exp();
    fx.flash = (fx.flash - dt).max(0.0);
    fx.hit_marker = (fx.hit_marker - dt).max(0.0);

    // Mid-slash, the machete is out whatever weapon is selected.
    let swinging = arsenal.swinging();
    let shown = if swinging {
        WeaponKind::Machete
    } else {
        arsenal.current
    };

    // How far through a reload the current gun is, 0 -> 1 (with the heart's
    // reload boost, as the gun itself counts it).
    let rules = tunings
        .get(&handle.0)
        .and_then(|t| t.rules(arsenal.current));
    let reload = match (arsenal.gun().reloading, rules) {
        (Some(elapsed), Some(rules)) if !swinging => {
            (elapsed / (rules.reload_time * boosts.reload)).clamp(0.0, 1.0)
        }
        _ => 0.0,
    };
    let (shift, turn) = reload_sway(arsenal.current, reload);
    for (piece, mut transform, mut visibility) in &mut pieces {
        let t = if piece.0 == arsenal.current {
            reload
        } else {
            0.0
        };
        let (pose, shown) = piece_pose(piece.1, t);
        *transform = pose;
        *visibility = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    // A freshly drawn weapon rises into place.
    let lowered = 1.0 - ease(arsenal.drawn_for / SWITCH_TIME);

    // Walking bob: a little figure-of-eight that speeds up with you.
    let speed = Vec2::new(status.velocity.x, status.velocity.z).length();
    let sway = if status.on_ground {
        (speed / 480.0).min(1.5)
    } else {
        0.0
    };
    *bob_phase += dt * (5.0 + speed * 0.01);
    let phase = *bob_phase;
    let bob = Vec3::new(phase.sin() * 0.4, -(phase * 2.0).sin().abs() * 0.35, 0.0) * sway;

    let kick = fx.kick;
    let (mut view, mut view_shown) = view.into_inner();
    // Nothing in hand on the title screen.
    *view_shown = if third_person.0 || !run.started {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    view.translation = REST + bob + shift + Vec3::new(0.0, kick * 0.6 - lowered * 10.0, kick * 4.0);
    view.rotation = Quat::from_euler(
        EulerRot::XYZ,
        kick * 0.32 - lowered * 0.6 + turn.x,
        turn.y,
        turn.z,
    );

    for (model, mut transform, mut visibility) in &mut models {
        // With the heart in hand, every weapon is put away.
        *visibility = if model.0 == shown && model.1 == look_for(shown, &upgrades) && !heart.held {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let (offset, _, _) = placement(model.0, model.1);
        if model.0 == WeaponKind::Machete && swinging {
            // A slash from upper right to lower left.
            let t = ease(arsenal.since_swing / SWING_TIME);
            let lerp = |a: f32, b: f32| a + (b - a) * t;
            transform.translation =
                offset + Vec3::new(lerp(4.0, -9.0), lerp(3.0, -3.0), lerp(1.0, -3.0));
            transform.rotation = Quat::from_euler(
                EulerRot::YXZ,
                lerp(1.0, -1.1),
                lerp(0.45, -0.5),
                lerp(-0.5, 0.9),
            );
        } else {
            transform.translation = offset;
            transform.rotation = if model.0 == WeaponKind::Machete {
                // Held ready: blade angled up and across.
                Quat::from_euler(EulerRot::YXZ, 0.2, 0.25, -0.2)
            } else {
                Quat::IDENTITY
            };
        }
    }

    let flashing = fx.flash > 0.0 && !swinging && !heart.held;
    let (offset, scale, muzzle) = placement(arsenal.current, look_for(arsenal.current, &upgrades));
    let (mut flash_at, mut flash_shown) = flash.into_inner();
    flash_at.translation = offset + muzzle * scale;
    // Bigger flash for bigger guns.
    flash_at.scale = Vec3::splat(match arsenal.current {
        WeaponKind::Revolver => 0.8,
        WeaponKind::TommyGun => 0.7,
        WeaponKind::Launcher => 1.1,
        _ => 1.0,
    });
    *flash_shown = if flashing {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    light.into_inner().intensity = if flashing { MUZZLE_LIGHT } else { 0.0 };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_weapon_has_a_model() {
        let stuff = test_stuff();
        for kind in WeaponKind::ALL {
            for look in [0, 1] {
                let boxes = parts(kind, look, &stuff);
                assert!(boxes.len() >= 6, "{kind:?}");
                assert!(boxes.iter().all(|(_, _, t)| t.scale.is_finite()));
            }
        }
    }

    /// Materials that can be told apart, without an app to make real ones.
    fn test_stuff() -> Stuff {
        let handle = |n: u128| Handle::from(bevy::asset::uuid::Uuid::from_u128(n));
        Stuff {
            metal: handle(1),
            steel: handle(2),
            wood: handle(3),
            leather: handle(4),
            olive: handle(5),
            red: handle(6),
            brass: handle(7),
            skin: handle(8),
            rot: handle(9),
            coat: handle(10),
        }
    }

    #[test]
    fn the_quad_launcher_has_four_barrels() {
        let stuff = test_stuff();
        let tubes = |look| {
            parts(WeaponKind::Launcher, look, &stuff)
                .iter()
                .filter(|(piece, material, t)| {
                    *piece == Piece::Frame && *material == stuff.olive && t.scale.z > 20.0
                })
                .count()
        };
        assert_eq!(tubes(0), 1);
        assert_eq!(tubes(1), 4);
    }

    #[test]
    fn the_heart_beats_lub_then_dub_and_rests_between() {
        let period = 60.0 / 60.0;
        assert_eq!(beat(0.0, period), (0.0, 0.0, 0.0));
        assert_eq!(beat(0.9, period), (0.0, 0.0, 0.0));
        let peak = |part: fn((f32, f32, f32)) -> f32| {
            (0..100)
                .map(|i| i as f32 * 0.01)
                .max_by(|&a, &b| part(beat(a, period)).total_cmp(&part(beat(b, period))))
                .unwrap()
        };
        let (atria, bulk, vessels) = (peak(|b| b.0), peak(|b| b.1), peak(|b| b.2));
        assert!(atria < bulk && bulk < vessels, "{atria} {bulk} {vessels}");
        // At 180 BPM the squeeze still fits in the beat.
        let fast = 60.0 / 180.0;
        assert_eq!(beat(fast - 0.01, fast), (0.0, 0.0, 0.0));
    }

    #[test]
    fn every_reload_starts_and_ends_at_rest() {
        for kind in WeaponKind::ALL {
            for t in [0.0, 1.0] {
                assert_eq!(reload_sway(kind, t), (Vec3::ZERO, Vec3::ZERO), "{kind:?}");
            }
            // Partway through, the gun is on the move.
            if kind != WeaponKind::Machete {
                assert_ne!(reload_sway(kind, 0.5).1, Vec3::ZERO, "{kind:?}");
            }
        }
        for piece in Piece::ALL {
            for t in [0.0, 1.0] {
                let (pose, shown) = piece_pose(piece, t);
                assert!(
                    pose.translation.distance(pivot(piece)) < 1e-4
                        && pose.rotation.angle_between(Quat::IDENTITY) < 1e-3,
                    "{piece:?} at {t}: {pose:?}"
                );
                // The fresh shells and rocket are inside the gun, out of sight.
                assert_eq!(
                    shown,
                    !matches!(piece, Piece::Shells | Piece::Rocket),
                    "{piece:?} at {t}"
                );
            }
        }
    }

    #[test]
    fn the_new_rounds_are_in_by_the_halfway_click() {
        // The click at halfway is the shells, drum and rocket going in.
        for piece in [Piece::Shells, Piece::Drum, Piece::Rocket] {
            let (pose, _) = piece_pose(piece, 0.56);
            let along = match piece {
                // The shells slide along the open barrels, to the hinge.
                Piece::Shells => {
                    let (barrels, _) = piece_pose(Piece::Barrels, 0.56);
                    pose.translation.distance(barrels.translation)
                }
                _ => pose.translation.distance(pivot(piece)),
            };
            assert!(along < 1e-3, "{piece:?}: {along}");
        }
    }
}
