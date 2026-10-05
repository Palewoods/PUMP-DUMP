//! Loads `assets/movement.ron` through Bevy's asset system (never `std::fs`, so it
//! will work on the web build too) and hot-reloads it when the file is saved.

use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::prelude::*;
use pf_movement::MovementTuning;

pub struct TuningPlugin;

impl Plugin for TuningPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<TuningAsset>()
            .register_asset_loader(TuningLoader)
            .add_systems(Startup, load)
            .add_systems(Update, report_reloads);
    }
}

/// Bevy note: assets must be Bevy types, but `pf-movement` must not depend on Bevy.
/// So we wrap its plain struct in a newtype that Bevy can store.
#[derive(Asset, TypePath, Deref)]
pub struct TuningAsset(pub MovementTuning);

#[derive(Resource)]
pub struct Tuning(pub Handle<TuningAsset>);

#[derive(TypePath)]
struct TuningLoader;

impl AssetLoader for TuningLoader {
    type Asset = TuningAsset;
    type Settings = ();
    type Error = Box<dyn std::error::Error + Send + Sync>;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<TuningAsset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(TuningAsset(ron::de::from_bytes(&bytes)?))
    }

    fn extensions(&self) -> &[&str] {
        &["movement.ron"]
    }
}

fn load(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(Tuning(assets.load("movement.ron")));
}

/// A typo in the file keeps the last good values; Bevy logs the parse error.
fn report_reloads(mut events: MessageReader<AssetEvent<TuningAsset>>) {
    for event in events.read() {
        if let AssetEvent::Modified { .. } = event {
            info!("movement.ron reloaded");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_tuning_parses() {
        let tuning: MovementTuning = ron::from_str(include_str!("../assets/movement.ron")).unwrap();
        assert!(tuning.capsule_height > 2.0 * tuning.capsule_radius);
        assert!(tuning.eye_height < tuning.capsule_height);
    }
}
