//! The characters, built from simple shapes.
//!
//! - The player: a zombie in a long trenchcoat and a fedora. Under the hat brim
//!   and behind the turned-up collar the face is pitch black: all you can make
//!   out are the glowing eyes. The hands are rotting grey-green.
//! - The enemies: human hunters in field jackets, with caps or helmets and
//!   rifles.
//!
//! Units are game units (~1 inch). The model stands on its origin (the feet),
//! 72 tall to match the movement capsule, and faces -Z.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

pub struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, make_kit)
            .add_systems(Update, animate_legs);
    }
}

/// Zombie skin: grey-green, sickly.
pub const SKIN: Color = Color::srgb(0.42, 0.5, 0.36);

/// How the zombie is dressed.
#[derive(Clone, Copy)]
pub struct Style {
    pub coat: Color,
    pub hat: Color,
    /// Colour of the glowing eyes. Values over 1 are fine: unlit, they're simply
    /// at full brightness.
    pub eyes: LinearRgba,
}

impl Style {
    /// The player: charcoal coat, black hat, pale yellow-green eyes.
    pub const PLAYER: Style = Style {
        coat: Color::srgb(0.14, 0.13, 0.12),
        hat: Color::srgb(0.1, 0.09, 0.08),
        eyes: LinearRgba::rgb(5.0, 6.0, 1.5),
    };
}

/// Shared meshes and materials for building characters.
#[derive(Resource)]
pub struct CharacterKit {
    cube: Handle<Mesh>,
    brim: Handle<Mesh>,
    crown: Handle<Mesh>,
    band: Handle<Mesh>,
    skin: Handle<StandardMaterial>,
    shadow: Handle<StandardMaterial>,
    trousers: Handle<StandardMaterial>,
    leather: Handle<StandardMaterial>,
    metal: Handle<StandardMaterial>,
    wood: Handle<StandardMaterial>,
}

impl CharacterKit {
    /// A unit cube: [`limb`] and [`slab`] stretch it into every box part.
    pub fn cube(&self) -> Handle<Mesh> {
        self.cube.clone()
    }

    pub fn skin(&self) -> Handle<StandardMaterial> {
        self.skin.clone()
    }
}

/// The root of a character model. Owners set `speed` (flat units/s) each frame
/// and the legs swing to match.
#[derive(Component, Default)]
pub struct Gait {
    pub speed: f32,
    pub grounded: bool,
    phase: f32,
}

/// A hip joint: rotating it swings the leg.
#[derive(Component)]
struct Hip {
    root: Entity,
    /// +1 or -1: legs swing opposite ways.
    side: f32,
}

fn make_kit(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let matte = |color: Color| StandardMaterial {
        base_color: color,
        perceptual_roughness: 1.0,
        reflectance: 0.05,
        ..default()
    };
    commands.insert_resource(CharacterKit {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        brim: meshes.add(Cylinder::new(10.0, 0.8)),
        crown: meshes.add(Cylinder::new(5.6, 6.5)),
        band: meshes.add(Cylinder::new(5.75, 1.4)),
        skin: materials.add(matte(SKIN)),
        // The face in the shadow of the brim: as near black as it gets.
        shadow: materials.add(StandardMaterial {
            base_color: Color::srgb(0.02, 0.02, 0.02),
            unlit: true,
            ..default()
        }),
        trousers: materials.add(matte(Color::srgb(0.12, 0.11, 0.1))),
        leather: materials.add(matte(Color::srgb(0.07, 0.05, 0.04))),
        metal: materials.add(StandardMaterial {
            base_color: Color::srgb(0.17, 0.17, 0.19),
            metallic: 0.7,
            perceptual_roughness: 0.45,
            ..default()
        }),
        wood: materials.add(matte(Color::srgb(0.36, 0.19, 0.09))),
    });
}

/// A box of `size` centred at `at`. For use with the kit's unit cube.
pub fn slab(at: Vec3, size: Vec3) -> Transform {
    Transform::from_translation(at).with_scale(size)
}

/// A box `thickness` square, stretched from `from` to `to`, like a limb or a
/// rope. For use with the kit's unit cube.
pub fn limb(from: Vec3, to: Vec3, thickness: Vec2) -> Transform {
    let span = to - from;
    Transform::from_translation(from + span * 0.5)
        .looking_to(span, Vec3::Y)
        .with_scale(thickness.extend(span.length()))
}

/// Build the zombie under a new entity at `transform` and return it. The root
/// carries a [`Gait`].
pub fn spawn_zombie(
    commands: &mut Commands,
    kit: &CharacterKit,
    materials: &mut Assets<StandardMaterial>,
    style: Style,
    transform: Transform,
) -> Entity {
    let matte = |color: Color| StandardMaterial {
        base_color: color,
        perceptual_roughness: 1.0,
        reflectance: 0.05,
        ..default()
    };
    let coat = materials.add(matte(style.coat));
    // A touch darker than the coat, for the belt and turned-up collar.
    let coat_dark = materials.add(matte(style.coat.darker(0.04)));
    let hat = materials.add(matte(style.hat));
    // Unlit materials ignore `emissive` and show `base_color` as is, so the glow
    // goes there. Brighter than 1 just clips to full brightness: as bright as it
    // gets, whatever the lighting.
    let eyes = materials.add(StandardMaterial {
        base_color: Color::LinearRgba(style.eyes),
        unlit: true,
        ..default()
    });

    let root = commands
        .spawn((transform, Visibility::default(), Gait::default()))
        .id();
    let part = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, at: Transform| {
        (
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            at,
            NotShadowCaster,
        )
    };

    commands.entity(root).with_children(|body| {
        // Legs hang from hip joints so they can swing.
        for side in [-1.0, 1.0] {
            body.spawn((
                Hip { root, side },
                Transform::from_xyz(5.0 * side, 30.0, 0.0),
                Visibility::default(),
            ))
            .with_children(|leg| {
                leg.spawn(part(
                    &kit.cube,
                    &kit.trousers,
                    slab(Vec3::new(0.0, -14.0, 0.0), Vec3::new(6.0, 28.0, 7.0)),
                ));
                leg.spawn(part(
                    &kit.cube,
                    &kit.leather,
                    slab(Vec3::new(0.0, -28.0, -1.5), Vec3::new(7.0, 4.0, 10.0)),
                ));
            });
        }

        // The trenchcoat: shoulders and chest, a long skirt to the knees, a belt.
        body.spawn(part(
            &kit.cube,
            &coat,
            slab(Vec3::new(0.0, 48.0, 0.0), Vec3::new(22.0, 20.0, 13.0)),
        ));
        body.spawn(part(
            &kit.cube,
            &coat,
            slab(Vec3::new(0.0, 26.0, 0.5), Vec3::new(23.5, 26.0, 14.5)),
        ));
        body.spawn(part(
            &kit.cube,
            &coat_dark,
            slab(Vec3::new(0.0, 38.5, 0.0), Vec3::new(23.0, 2.5, 13.8)),
        ));
        // Collar turned up round the face.
        for side in [-1.0f32, 1.0] {
            let collar = slab(Vec3::new(5.8 * side, 60.0, -3.2), Vec3::new(7.0, 9.0, 2.5))
                .with_rotation(Quat::from_rotation_z(0.28 * side));
            body.spawn(part(&kit.cube, &coat_dark, collar));
        }
        body.spawn(part(
            &kit.cube,
            &coat_dark,
            slab(Vec3::new(0.0, 60.0, 4.5), Vec3::new(17.0, 10.0, 3.0)),
        ));

        // The face: black, apart from the eyes.
        body.spawn(part(
            &kit.cube,
            &kit.shadow,
            slab(Vec3::new(0.0, 62.5, 0.0), Vec3::new(9.0, 10.0, 9.0)),
        ));
        // Big enough to still show up as pixels at a distance: they're the one
        // thing you're meant to see.
        for side in [-1.0, 1.0] {
            body.spawn(part(
                &kit.cube,
                &eyes,
                slab(Vec3::new(2.3 * side, 63.5, -4.6), Vec3::new(2.6, 1.5, 0.5)),
            ));
        }

        // Fedora, pulled down low and tipped forward.
        let tip = Quat::from_rotation_x(-0.1);
        body.spawn(part(
            &kit.brim,
            &hat,
            Transform::from_xyz(0.0, 67.2, 0.0).with_rotation(tip),
        ));
        body.spawn(part(
            &kit.crown,
            &hat,
            Transform::from_xyz(0.0, 70.8, 0.6).with_rotation(tip),
        ));
        body.spawn(part(
            &kit.band,
            &kit.leather,
            Transform::from_xyz(0.0, 68.4, 0.5).with_rotation(tip),
        ));

        // Arms in coat sleeves, out in front holding a shotgun at the hip.
        let right_hand = Vec3::new(6.0, 44.0, -14.0);
        let left_hand = Vec3::new(-2.0, 47.0, -24.0);
        body.spawn(part(
            &kit.cube,
            &coat,
            limb(Vec3::new(11.5, 55.0, 0.0), right_hand, Vec2::splat(5.0)),
        ));
        body.spawn(part(
            &kit.cube,
            &coat,
            limb(Vec3::new(-11.5, 55.0, 0.0), left_hand, Vec2::splat(5.0)),
        ));
        body.spawn(part(
            &kit.cube,
            &kit.skin,
            slab(right_hand, Vec3::new(3.6, 4.0, 4.5)),
        ));
        body.spawn(part(
            &kit.cube,
            &kit.skin,
            slab(left_hand, Vec3::new(3.6, 3.6, 4.5)),
        ));
        // The gun: barrels forward past the left hand, stock back by the hip.
        let muzzle = Vec3::new(-3.5, 48.5, -40.0);
        let grip = Vec3::new(5.0, 45.0, -10.0);
        body.spawn(part(
            &kit.cube,
            &kit.metal,
            limb(grip, muzzle, Vec2::new(2.6, 2.4)),
        ));
        body.spawn(part(
            &kit.cube,
            &kit.wood,
            limb(Vec3::new(7.5, 42.0, 0.0), grip, Vec2::new(2.4, 3.6)),
        ));
    });
    root
}

/// How a human hunter looks.
#[derive(Clone, Copy)]
pub struct HumanStyle {
    pub jacket: Color,
    pub skin: Color,
    pub hair: Color,
    /// A steel helmet; otherwise a cap.
    pub helmet: bool,
}

/// The parts of a hunter its owner changes: the jacket (glows white for a
/// moment when hit) and the glow at the rifle's muzzle (builds up before a shot).
#[derive(Component)]
pub struct HumanLook {
    pub jacket: Handle<StandardMaterial>,
    pub muzzle_glow: Handle<StandardMaterial>,
}

/// Where a hunter's rifle muzzle is, relative to its feet (facing -Z), for shots
/// to start at. Matches the model below.
pub const RIFLE_MUZZLE: Vec3 = Vec3::new(-1.0, 52.5, -42.0);
/// Where a hunter's eyes are, relative to its feet.
pub const HUMAN_EYE: Vec3 = Vec3::new(0.0, 64.0, 0.0);
/// The muzzle glow when idle (unlit, so this is its colour as seen).
pub const MUZZLE_IDLE: LinearRgba = LinearRgba::rgb(0.25, 0.08, 0.03);

/// Build a human hunter under a new entity at `transform` and return it: field
/// jacket, webbing belt, cap or helmet, a plain human face, and a rifle held at
/// the shoulder. The root carries a [`Gait`] and a [`HumanLook`].
pub fn spawn_human(
    commands: &mut Commands,
    kit: &CharacterKit,
    materials: &mut Assets<StandardMaterial>,
    style: HumanStyle,
    transform: Transform,
) -> Entity {
    let matte = |color: Color| StandardMaterial {
        base_color: color,
        perceptual_roughness: 1.0,
        reflectance: 0.05,
        ..default()
    };
    let jacket = materials.add(matte(style.jacket));
    let webbing = materials.add(matte(style.jacket.darker(0.08)));
    let skin = materials.add(matte(style.skin));
    let skin_shade = materials.add(matte(style.skin.darker(0.12)));
    let hair = materials.add(matte(style.hair));
    let headgear = materials.add(matte(if style.helmet {
        Color::srgb(0.22, 0.25, 0.17)
    } else {
        style.jacket.darker(0.05)
    }));
    let dark = materials.add(matte(Color::srgb(0.05, 0.04, 0.04)));
    let lips = materials.add(matte(Color::srgb(0.35, 0.16, 0.13)));
    let muzzle_glow = materials.add(StandardMaterial {
        base_color: Color::LinearRgba(MUZZLE_IDLE),
        unlit: true,
        ..default()
    });

    let root = commands
        .spawn((
            transform,
            Visibility::default(),
            Gait::default(),
            HumanLook {
                jacket: jacket.clone(),
                muzzle_glow: muzzle_glow.clone(),
            },
        ))
        .id();
    let part = |material: &Handle<StandardMaterial>, at: Transform| {
        (
            Mesh3d(kit.cube.clone()),
            MeshMaterial3d(material.clone()),
            at,
            NotShadowCaster,
        )
    };

    commands.entity(root).with_children(|body| {
        for side in [-1.0, 1.0] {
            body.spawn((
                Hip { root, side },
                Transform::from_xyz(4.8 * side, 31.0, 0.0),
                Visibility::default(),
            ))
            .with_children(|leg| {
                leg.spawn(part(
                    &kit.trousers,
                    slab(Vec3::new(0.0, -14.5, 0.0), Vec3::new(6.5, 29.0, 7.0)),
                ));
                leg.spawn(part(
                    &kit.leather,
                    slab(Vec3::new(0.0, -29.0, -1.5), Vec3::new(7.0, 4.0, 10.0)),
                ));
            });
        }

        // Field jacket, webbing belt with pouches.
        body.spawn(part(
            &jacket,
            slab(Vec3::new(0.0, 46.0, 0.0), Vec3::new(20.0, 22.0, 11.0)),
        ));
        body.spawn(part(
            &jacket,
            slab(Vec3::new(0.0, 33.0, 0.0), Vec3::new(20.5, 6.0, 11.5)),
        ));
        body.spawn(part(
            &webbing,
            slab(Vec3::new(0.0, 36.5, 0.0), Vec3::new(21.0, 2.6, 12.0)),
        ));
        for side in [-1.0, 1.0] {
            body.spawn(part(
                &webbing,
                slab(Vec3::new(5.0 * side, 36.5, -6.3), Vec3::new(4.0, 4.0, 2.0)),
            ));
        }

        // Neck and head, with a plain face: eyes, brows, nose, mouth.
        body.spawn(part(
            &skin,
            slab(Vec3::new(0.0, 58.0, 0.0), Vec3::new(5.0, 3.0, 5.0)),
        ));
        body.spawn(part(
            &skin,
            slab(Vec3::new(0.0, 63.5, 0.0), Vec3::new(9.0, 10.0, 9.0)),
        ));
        for side in [-1.0, 1.0] {
            body.spawn(part(
                &dark,
                slab(Vec3::new(2.0 * side, 64.0, -4.6), Vec3::new(1.4, 0.9, 0.4)),
            ));
            body.spawn(part(
                &hair,
                slab(Vec3::new(2.0 * side, 65.4, -4.6), Vec3::new(2.2, 0.6, 0.4)),
            ));
        }
        body.spawn(part(
            &skin_shade,
            slab(Vec3::new(0.0, 62.6, -4.9), Vec3::new(1.2, 2.0, 1.0)),
        ));
        body.spawn(part(
            &lips,
            slab(Vec3::new(0.0, 60.4, -4.6), Vec3::new(3.0, 0.6, 0.4)),
        ));
        // Hair at the back and sides, under the headgear.
        body.spawn(part(
            &hair,
            slab(Vec3::new(0.0, 65.0, 1.2), Vec3::new(9.4, 6.0, 7.0)),
        ));

        if style.helmet {
            body.spawn(part(
                &headgear,
                slab(Vec3::new(0.0, 68.6, 0.2), Vec3::new(10.6, 4.4, 10.6)),
            ));
            body.spawn(part(
                &headgear,
                slab(Vec3::new(0.0, 66.8, 0.2), Vec3::new(11.8, 1.0, 11.8)),
            ));
        } else {
            body.spawn(part(
                &headgear,
                slab(Vec3::new(0.0, 69.0, 0.4), Vec3::new(9.6, 3.6, 9.6)),
            ));
            body.spawn(part(
                &headgear,
                slab(Vec3::new(0.0, 67.4, -5.6), Vec3::new(8.0, 0.6, 4.0)),
            ));
        }

        // Arms raising a rifle to the shoulder.
        let grip = Vec3::new(5.0, 50.0, -8.0);
        let fore = Vec3::new(-1.5, 51.0, -22.0);
        body.spawn(part(
            &jacket,
            limb(Vec3::new(10.5, 55.0, 0.0), grip, Vec2::splat(4.8)),
        ));
        body.spawn(part(
            &jacket,
            limb(Vec3::new(-10.5, 55.0, 0.0), fore, Vec2::splat(4.8)),
        ));
        body.spawn(part(&skin, slab(grip, Vec3::new(3.4, 3.8, 4.0))));
        body.spawn(part(&skin, slab(fore, Vec3::new(3.4, 3.4, 4.0))));
        // Rifle: stock in the shoulder, body, magazine, long barrel.
        let shoulder = Vec3::new(6.5, 53.5, 2.0);
        let action = Vec3::new(2.5, 52.0, -14.0);
        body.spawn(part(
            &kit.wood,
            limb(shoulder, Vec3::new(4.5, 52.5, -6.0), Vec2::new(2.6, 3.6)),
        ));
        body.spawn(part(
            &kit.metal,
            limb(Vec3::new(4.5, 52.5, -6.0), action, Vec2::new(2.8, 3.2)),
        ));
        body.spawn(part(
            &kit.metal,
            slab(Vec3::new(3.0, 48.5, -12.0), Vec3::new(1.8, 5.0, 3.0)),
        ));
        body.spawn(part(
            &kit.wood,
            limb(action, Vec3::new(-0.5, 51.5, -27.0), Vec2::new(2.6, 2.6)),
        ));
        body.spawn(part(
            &kit.metal,
            limb(
                Vec3::new(-0.5, 52.4, -27.0),
                RIFLE_MUZZLE,
                Vec2::new(1.2, 1.2),
            ),
        ));
        body.spawn(part(
            &muzzle_glow,
            slab(RIFLE_MUZZLE + Vec3::new(0.0, 0.0, -0.8), Vec3::splat(2.2)),
        ));
    });
    root
}

/// Swing the legs: a stride that speeds up with the walker, still when stopped
/// or in the air.
fn animate_legs(
    time: Res<Time>,
    mut gaits: Query<&mut Gait>,
    mut hips: Query<(&Hip, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for mut gait in &mut gaits {
        // About one stride per 60 units travelled.
        gait.phase += dt * gait.speed / 60.0 * std::f32::consts::PI;
    }
    for (hip, mut transform) in &mut hips {
        let Ok(gait) = gaits.get(hip.root) else {
            continue;
        };
        let reach = if gait.grounded {
            (gait.speed / 300.0).min(1.0) * 0.6
        } else {
            0.25 // legs trail a little in the air
        };
        transform.rotation = Quat::from_rotation_x(gait.phase.sin() * reach * hip.side);
    }
}
