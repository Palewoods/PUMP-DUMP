//! Sound effects made in code at startup, so the game needs no audio files.
//!
//! Bevy note: Bevy plays any asset type that implements `Decodable` (a stream of
//! samples). [`Sfx`] is a short pre-made list of samples.

use std::f32::consts::TAU;
use std::sync::Arc;
use std::time::Duration;

use bevy::audio::{AddAudioSource, ChannelCount, SampleRate, Source};
use bevy::prelude::*;

pub struct SfxPlugin;

impl Plugin for SfxPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<Sfx>()
            .add_systems(Startup, make_sounds);
    }
}

const SAMPLE_RATE: u32 = 44_100;

/// The game's sounds, ready to play with [`play`].
#[derive(Resource)]
pub struct Sounds {
    pub shotgun: Handle<Sfx>,
    /// Breaking the shotgun open.
    pub open: Handle<Sfx>,
    /// Shells going in.
    pub load: Handle<Sfx>,
    /// Snapping it shut.
    pub close: Handle<Sfx>,
    /// A pellet hitting a target.
    pub hit: Handle<Sfx>,
    /// Dashing, and the grappling hook flying out.
    pub whoosh: Handle<Sfx>,
    /// The grappling hook biting into something.
    pub clank: Handle<Sfx>,
    /// A ground slam landing, or an enemy bursting apart.
    pub thud: Handle<Sfx>,
    /// An enemy firing.
    pub zap: Handle<Sfx>,
    /// The player getting hit.
    pub hurt: Handle<Sfx>,
    pub revolver: Handle<Sfx>,
    /// One tommy gun round: short, so a burst rattles.
    pub tommy: Handle<Sfx>,
    /// A rocket leaving the launcher.
    pub launch: Handle<Sfx>,
    /// A rocket exploding.
    pub boom: Handle<Sfx>,
    /// The machete cutting the air...
    pub swish: Handle<Sfx>,
    /// ...and into something.
    pub chop: Handle<Sfx>,
}

/// Play a sound once.
pub fn play(commands: &mut Commands, sound: &Handle<Sfx>) {
    commands.spawn((AudioPlayer(sound.clone()), PlaybackSettings::DESPAWN));
}

/// A short mono sound, as samples in -1..1.
#[derive(Asset, TypePath, Clone)]
pub struct Sfx {
    // Rust note: `Arc<[f32]>` is a shared, read-only list, so each playback
    // borrows the samples instead of copying them.
    samples: Arc<[f32]>,
}

pub struct SfxDecoder {
    samples: Arc<[f32]>,
    next: usize,
}

impl Iterator for SfxDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.samples.get(self.next).copied();
        self.next += 1;
        sample
    }
}

impl Source for SfxDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(1).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(SAMPLE_RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f32(
            self.samples.len() as f32 / SAMPLE_RATE as f32,
        ))
    }
}

impl Decodable for Sfx {
    type Decoder = SfxDecoder;

    fn decoder(&self) -> SfxDecoder {
        SfxDecoder {
            samples: self.samples.clone(),
            next: 0,
        }
    }
}

fn make_sounds(mut commands: Commands, mut sfx: ResMut<Assets<Sfx>>) {
    let mut add = |samples: Vec<f32>| {
        sfx.add(Sfx {
            samples: samples.into(),
        })
    };
    commands.insert_resource(Sounds {
        shotgun: add(shotgun_blast()),
        open: add(click(0.05, 1900.0, 0.5, 3)),
        load: add(click(0.06, 520.0, 0.35, 7)),
        close: add(click(0.08, 1300.0, 0.6, 11)),
        hit: add(click(0.04, 260.0, 0.35, 13)),
        whoosh: add(whoosh()),
        clank: add(click(0.12, 900.0, 0.7, 17)),
        thud: add(thud()),
        zap: add(zap()),
        hurt: add(hurt()),
        revolver: add(gunshot(0.6, 0.9, 0.3, 110.0, 9.0, 0x0A11_CE01)),
        tommy: add(gunshot(0.18, 0.7, 0.25, 140.0, 26.0, 0x70AA_5EED)),
        launch: add(whoosh()),
        boom: add(gunshot(1.4, 0.3, 0.35, 45.0, 3.5, 0x00B0_0000)),
        swish: add(whoosh()),
        chop: add(click(0.1, 300.0, 0.8, 23)),
    });
}

/// Repeatable white noise (xorshift), so every run sounds the same.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

fn samples(seconds: f32) -> impl Iterator<Item = f32> {
    (0..(seconds * SAMPLE_RATE as f32) as usize).map(|i| i as f32 / SAMPLE_RATE as f32)
}

/// A big low boom: muffled noise with a sharp attack and a slow tail, over a
/// falling sub-bass thump.
fn shotgun_blast() -> Vec<f32> {
    let mut noise = Noise(0x9E37_79B9);
    let mut muffled = 0.0;
    samples(0.9)
        .map(|t| {
            // One-pole low-pass: each sample moves part-way towards the noise.
            muffled += 0.18 * (noise.next() - muffled);
            let attack = (t / 0.003).min(1.0);
            let crack = noise.next() * (-t * 60.0).exp() * 0.5;
            let body = muffled * 2.2 * (-t * 7.0).exp();
            let thump = (TAU * (70.0 - 40.0 * t) * t).sin() * (-t * 9.0).exp();
            ((crack + body + thump) * attack * 0.75).clamp(-1.0, 1.0)
        })
        .collect()
}

/// A rush of air: noise that swells and fades, brightening as it goes.
fn whoosh() -> Vec<f32> {
    let mut noise = Noise(0x0BAD_F00D);
    let mut low = 0.0;
    let seconds = 0.3;
    samples(seconds)
        .map(|t| {
            // The low-pass opens up over the sound, so it gets brighter.
            low += (0.04 + 0.3 * t / seconds) * (noise.next() - low);
            // Rises and falls smoothly, all the way to silence at both ends.
            let swell = (t / seconds * std::f32::consts::PI).sin().powi(4);
            (low * 2.8 * swell).clamp(-1.0, 1.0)
        })
        .collect()
}

/// A heavy landing: a deep falling thump with a bit of crunch.
fn thud() -> Vec<f32> {
    let mut noise = Noise(0x5EED_1234);
    let mut low = 0.0;
    samples(0.5)
        .map(|t| {
            low += 0.12 * (noise.next() - low);
            let thump = (TAU * (90.0 - 60.0 * t) * t).sin() * (-t * 10.0).exp();
            let crunch = low * 1.5 * (-t * 18.0).exp();
            ((thump + crunch) * 0.9).clamp(-1.0, 1.0)
        })
        .collect()
}

/// An enemy shot: a falling, buzzing tone.
fn zap() -> Vec<f32> {
    let mut noise = Noise(0x07A9_3E11);
    let mut phase = 0.0f32;
    samples(0.3)
        .map(|t| {
            // Pitch falls from 900 Hz to 250 Hz; part square wave, for buzz.
            phase += TAU * (900.0 - 2200.0 * t).max(250.0) / SAMPLE_RATE as f32;
            let buzz = phase.sin().signum() * 0.35 + phase.sin() * 0.3;
            let fade = (-t * 14.0).exp();
            ((buzz + noise.next() * 0.15) * fade * 0.6).clamp(-1.0, 1.0)
        })
        .collect()
}

/// Getting hit: a dull punch with a crunch.
fn hurt() -> Vec<f32> {
    let mut noise = Noise(0x0F00_D00D);
    let mut low = 0.0;
    samples(0.3)
        .map(|t| {
            low += 0.2 * (noise.next() - low);
            let punch = (TAU * (140.0 - 200.0 * t).max(50.0) * t).sin() * (-t * 16.0).exp();
            ((punch + low * 1.6 * (-t * 22.0).exp()) * 0.85).clamp(-1.0, 1.0)
        })
        .collect()
}

/// A gunshot: a sharp crack over a muffled body and a falling thump.
/// `crack` and `body` set how much of each; `thump` is the thump's starting
/// pitch (Hz); `decay` how fast it all dies away (higher is shorter).
fn gunshot(seconds: f32, crack: f32, body: f32, thump: f32, decay: f32, seed: u32) -> Vec<f32> {
    let mut noise = Noise(seed | 1);
    let mut muffled = 0.0;
    samples(seconds)
        .map(|t| {
            muffled += 0.2 * (noise.next() - muffled);
            let attack = (t / 0.002).min(1.0);
            let snap = noise.next() * (-t * 70.0).exp() * crack;
            let rumble = muffled * 2.4 * body * (-t * decay).exp();
            let boom =
                (TAU * (thump * (1.0 - 0.5 * t / seconds)) * t).sin() * (-t * decay * 1.3).exp();
            ((snap + rumble + boom * 0.8) * attack * 0.75).clamp(-1.0, 1.0)
        })
        .collect()
}

/// A short mechanical click: a noise tick plus a quickly fading ring at `pitch`.
fn click(seconds: f32, pitch: f32, volume: f32, seed: u32) -> Vec<f32> {
    let mut noise = Noise(0x1234_5678 ^ seed.wrapping_mul(0x0101_0101));
    samples(seconds)
        .map(|t| {
            let tick = noise.next() * (-t * 300.0).exp();
            let ring = (TAU * pitch * t).sin() * (-t * 60.0).exp();
            ((tick + ring * 0.6) * volume).clamp(-1.0, 1.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_stay_in_range_and_fade_out() {
        for sound in [
            shotgun_blast(),
            click(0.05, 1900.0, 0.5, 3),
            whoosh(),
            thud(),
            zap(),
            hurt(),
            gunshot(0.6, 0.9, 0.3, 110.0, 9.0, 1),
            gunshot(1.4, 0.3, 0.35, 45.0, 3.5, 2),
        ] {
            assert!(sound.iter().all(|s| s.abs() <= 1.0));
            let tail = &sound[sound.len() * 9 / 10..];
            assert!(tail.iter().all(|s| s.abs() < 0.05), "doesn't fade out");
        }
    }
}
