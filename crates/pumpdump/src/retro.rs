//! The retro look: the 3D world is drawn into a small image (a few hundred pixels
//! tall), then stretched over the window with hard pixel edges, a reduced colour
//! palette and ordered dithering.
//!
//! Bevy note: the player's cameras render into [`RetroScreen`]'s image instead of
//! the window. A separate 2D camera owns the window and draws the UI: the
//! full-window [`RetroMaterial`] node showing that image, with the HUD on top.

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::ui::IsDefaultUiCamera;
use bevy::window::PrimaryWindow;

pub struct RetroPlugin;

impl Plugin for RetroPlugin {
    fn build(&self, app: &mut App) {
        // The image has to exist before the player's cameras spawn and point at
        // it, so it's made here rather than in a Startup system.
        let size = UVec2::new(PIXEL_HEIGHT * 16 / 9, PIXEL_HEIGHT);
        let image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        app.insert_resource(RetroScreen { image })
            .add_plugins(UiMaterialPlugin::<RetroMaterial>::default())
            .add_systems(Startup, spawn_screen)
            .add_systems(Update, fit_to_window);
    }
}

/// Height of the low-resolution image, pixels. Width follows the window's shape.
const PIXEL_HEIGHT: u32 = 270;
/// Brightness steps per colour channel after quantising. Fewer is crunchier.
const COLOUR_LEVELS: f32 = 14.0;
/// How strongly the dither pattern breaks up colour bands, 0..1.
const DITHER: f32 = 0.85;

/// Render layer for the first-person weapon, drawn by its own camera on top of
/// the world so it never clips into walls.
pub const VIEW_MODEL_LAYER: usize = 1;

/// The low-resolution image the 3D cameras draw into.
#[derive(Resource)]
pub struct RetroScreen {
    pub image: Handle<Image>,
}

impl RetroScreen {
    /// Where a 3D camera should render to be part of the retro screen.
    pub fn target(&self) -> RenderTarget {
        RenderTarget::Image(self.image.clone().into())
    }
}

/// Draws the low-resolution image over the whole window with the palette and
/// dither applied (see `assets/shaders/retro.wgsl`).
#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub struct RetroMaterial {
    /// x = colour levels per channel, y = dither strength. A `Vec4` because web
    /// builds need 16-byte-aligned uniforms.
    #[uniform(0)]
    settings: Vec4,
    #[texture(1)]
    #[sampler(2)]
    screen: Handle<Image>,
}

impl UiMaterial for RetroMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/retro.wgsl".into()
    }
}

fn spawn_screen(
    mut commands: Commands,
    screen: Res<RetroScreen>,
    mut materials: ResMut<Assets<RetroMaterial>>,
) {
    // Owns the window: clears it and draws the UI.
    commands.spawn((
        Camera2d,
        Camera {
            order: 10,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        Msaa::Off,
        IsDefaultUiCamera,
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        MaterialNode(materials.add(RetroMaterial {
            settings: Vec4::new(COLOUR_LEVELS, DITHER, 0.0, 0.0),
            screen: screen.image.clone(),
        })),
        // Behind every other bit of UI (the HUD).
        GlobalZIndex(-1),
    ));
}

/// Keeps the image's width matched to the window's shape, so pixels stay square.
fn fit_to_window(
    window: Single<&Window, With<PrimaryWindow>>,
    screen: Res<RetroScreen>,
    mut images: ResMut<Assets<Image>>,
) {
    let (w, h) = (window.width(), window.height());
    if w < 1.0 || h < 1.0 {
        return; // minimised
    }
    let width = ((PIXEL_HEIGHT as f32 * w / h).round() as u32).max(1);
    // Only touch the image when it actually changes: `get_mut` marks it as
    // modified, which re-uploads it to the GPU.
    let current = images.get(&screen.image).map(|i| i.width());
    if current.is_some_and(|c| c != width)
        && let Some(mut image) = images.get_mut(&screen.image)
    {
        image.resize(Extent3d {
            width,
            height: PIXEL_HEIGHT,
            depth_or_array_layers: 1,
        });
    }
}
