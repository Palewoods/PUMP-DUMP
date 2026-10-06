//! What the movement abilities look and sound like: the grappling hook's rope,
//! the dash charge meter, and the whoosh, clank and thud of dashing, grappling
//! and slamming. The abilities themselves live in `pumpdump-movement`; this only
//! shows them.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::player::{PlayerCamera, PlayerStatus};
use crate::sfx::{self, Sounds};

pub struct AbilitiesPlugin;

impl Plugin for AbilitiesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_rope, spawn_meter))
            .add_systems(Update, (draw_rope, update_meter, play_sounds));
    }
}

/// Where the rope leaves from, relative to the eye: low on the left, the
/// opposite side to the shotgun.
const ROPE_HAND: Vec3 = Vec3::new(-6.0, -6.0, -10.0);
const ROPE_THICKNESS: f32 = 1.2;

#[derive(Component)]
struct Rope;

#[derive(Component)]
struct Hook;

#[derive(Component)]
struct DashMeter;

fn spawn_rope(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let metal = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.5, 0.42),
        metallic: 0.6,
        perceptual_roughness: 0.6,
        ..default()
    });
    // A 1-unit cube, stretched each frame from the hand to the hook.
    commands.spawn((
        Rope,
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(metal.clone()),
        Transform::default(),
        Visibility::Hidden,
        NotShadowCaster,
    ));
    commands.spawn((
        Hook,
        Mesh3d(meshes.add(Cuboid::new(5.0, 5.0, 5.0))),
        MeshMaterial3d(metal),
        Transform::default(),
        Visibility::Hidden,
        NotShadowCaster,
    ));
}

// Bevy note: these filters (`With`/`Without`) tell Bevy the three queries never
// touch the same entity, which it needs to hand out the `&mut Transform`s
// safely. Clippy finds the types long; that's normal for Bevy queries.
#[allow(clippy::type_complexity)]
fn draw_rope(
    status: Res<PlayerStatus>,
    eye: Single<&Transform, (With<PlayerCamera>, Without<Rope>, Without<Hook>)>,
    rope: Single<(&mut Transform, &mut Visibility), (With<Rope>, Without<Hook>)>,
    hook: Single<(&mut Transform, &mut Visibility), (With<Hook>, Without<Rope>)>,
) {
    let (mut rope, mut rope_shown) = rope.into_inner();
    let (mut hook, mut hook_shown) = hook.into_inner();
    let Some(grapple) = status.grapple else {
        *rope_shown = Visibility::Hidden;
        *hook_shown = Visibility::Hidden;
        return;
    };
    let tip = grapple.tip(eye.translation);
    let hand = eye.transform_point(ROPE_HAND);
    let span = tip - hand;
    let length = span.length();
    if length < 1.0 {
        return;
    }
    // Rust note: `Transform::looking_to` points -Z along `span`; our cube is
    // stretched along Z, so it lies along the rope.
    *rope = Transform::from_translation(hand + span * 0.5)
        .looking_to(span, Vec3::Y)
        .with_scale(Vec3::new(ROPE_THICKNESS, ROPE_THICKNESS, length));
    hook.translation = tip;
    hook.rotation = rope.rotation;
    *rope_shown = Visibility::Visible;
    *hook_shown = Visibility::Visible;
}

fn spawn_meter(mut commands: Commands) {
    commands.spawn((
        DashMeter,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(24.0),
            ..default()
        },
        TextColor(Color::srgb(0.65, 0.85, 0.95)),
        Node {
            position_type: PositionType::Absolute,
            left: px(28),
            bottom: px(22),
            ..default()
        },
    ));
}

/// "DASH [#][#][-]": full charges, and how far the next one has refilled.
fn update_meter(status: Res<PlayerStatus>, mut text: Single<&mut Text, With<DashMeter>>) {
    let charges = status.max_stamina.round() as usize;
    let cells: String = (0..charges)
        .map(|i| {
            let fill = (status.stamina - i as f32).clamp(0.0, 1.0);
            if fill >= 1.0 {
                "[#]"
            } else if fill >= 0.5 {
                "[+]"
            } else if fill > 0.0 {
                "[-]"
            } else {
                "[ ]"
            }
        })
        .collect();
    text.0 = format!("DASH {cells}");
}

/// Sounds for the moments the movement code doesn't announce: watch the status
/// for things starting.
fn play_sounds(
    mut commands: Commands,
    status: Res<PlayerStatus>,
    sounds: Option<Res<Sounds>>,
    // (hook out, hook stuck, dashing, slamming) last frame.
    mut before: Local<(bool, bool, bool, bool)>,
) {
    let now = (
        status.grapple.is_some(),
        status.grapple.is_some_and(|g| g.attached),
        status.dashing,
        status.slamming,
    );
    if let Some(sounds) = sounds {
        let started = |then: bool, now: bool| now && !then;
        if started(before.0, now.0) {
            sfx::play(&mut commands, &sounds.whoosh);
        }
        if started(before.1, now.1) {
            sfx::play(&mut commands, &sounds.clank);
        }
        if started(before.2, now.2) {
            sfx::play(&mut commands, &sounds.whoosh);
        }
        // A slam only ends by hitting the ground.
        if before.3 && !now.3 {
            sfx::play(&mut commands, &sounds.thud);
        }
    }
    *before = now;
}
