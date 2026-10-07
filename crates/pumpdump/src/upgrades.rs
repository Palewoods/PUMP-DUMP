//! Upgrades. Clear a level and you pick one: the next level of a weapon, or of
//! your perk (if you have one). Each has two levels. They last for the rest of
//! the run through the levels, and start again from nothing when you start
//! afresh from the title screen.
//!
//! Some upgrades change a weapon's numbers (see [`Upgrades::weapons_tuning`]),
//! others what it does: the weapon code asks [`Upgrades::has`].

use bevy::prelude::*;

use crate::heart::{HeartTuning, Perk};
use crate::weapon::{WeaponKind, WeaponsTuning};

pub struct UpgradesPlugin;

impl Plugin for UpgradesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Upgrades>();
    }
}

/// How many levels each weapon and perk can be upgraded.
pub const MAX_LEVEL: u8 = 2;

/// Machete, level 2: every hit heals this much...
pub const BLOODLETTER_HEAL: f32 = 10.0;
/// ...and puts this much blood back in your heart.
pub const BLOODLETTER_BLOOD: f32 = 0.08;
/// Revolver, level 2: a bullet that hits bounces on to the nearest other
/// target within this distance.
pub const RICOCHET_RANGE: f32 = 1200.0;
/// Shotgun, level 2: each shot shoves you backwards this hard, units/s.
pub const BOOMSTICK_SHOVE: f32 = 650.0;
/// Tommy gun, level 2: bullets go through up to this many targets.
pub const PIERCE: usize = 3;
/// Rocket launcher, level 1: rockets per shot, and each one's share of the
/// damage and the shove.
pub const QUAD_ROCKETS: usize = 4;
pub const QUAD_DAMAGE: f32 = 0.55;
/// Rocket launcher, level 2: rockets find targets within this angle of where
/// they're going (degrees) and turn towards them this fast (radians/second).
pub const HOMING_CONE: f32 = 70.0;
pub const HOMING_TURN: f32 = 4.0;
/// Bloodlust, level 2: blood back in your heart per kill.
pub const FRENZY_BLOOD: f32 = 0.2;

/// What's been upgraded this run: a level (0 to [`MAX_LEVEL`]) per weapon, by
/// slot, and for the perk.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct Upgrades {
    pub weapons: [u8; 5],
    pub perk: u8,
    /// Which perk the perk level belongs to: a different perk starts again.
    pub perk_for: Option<Perk>,
}

/// One thing you could upgrade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Offer {
    Weapon(WeaponKind),
    Perk(Perk),
}

impl Upgrades {
    pub fn level(&self, kind: WeaponKind) -> u8 {
        self.weapons[kind.slot()]
    }

    /// Whether `kind` has been upgraded to at least `level`.
    pub fn has(&self, kind: WeaponKind, level: u8) -> bool {
        self.level(kind) >= level
    }

    /// What can still be upgraded: every weapon not yet maxed out, then the
    /// perk, if there is one.
    pub fn offers(&self, perk: Option<Perk>) -> Vec<Offer> {
        let mut offers: Vec<Offer> = WeaponKind::ALL
            .into_iter()
            .filter(|&kind| self.level(kind) < MAX_LEVEL)
            .map(Offer::Weapon)
            .collect();
        if let Some(perk) = perk
            && self.perk_level(perk) < MAX_LEVEL
        {
            offers.push(Offer::Perk(perk));
        }
        offers
    }

    fn perk_level(&self, perk: Perk) -> u8 {
        if self.perk_for == Some(perk) {
            self.perk
        } else {
            0
        }
    }

    /// Take an upgrade. Returns its name.
    pub fn take(&mut self, offer: Offer) -> &'static str {
        match offer {
            Offer::Weapon(kind) => {
                let level = &mut self.weapons[kind.slot()];
                *level = (*level + 1).min(MAX_LEVEL);
                weapon_upgrade(kind, *level).0
            }
            Offer::Perk(perk) => {
                self.perk = (self.perk_level(perk) + 1).min(MAX_LEVEL);
                self.perk_for = Some(perk);
                perk_upgrade(perk, self.perk).0
            }
        }
    }

    /// The next upgrade an offer would give: its name and what it does, and
    /// which level it is.
    pub fn describe(&self, offer: Offer) -> (&'static str, &'static str, u8) {
        match offer {
            Offer::Weapon(kind) => {
                let next = self.level(kind) + 1;
                let (name, what) = weapon_upgrade(kind, next);
                (name, what, next)
            }
            Offer::Perk(perk) => {
                let next = self.perk_level(perk) + 1;
                let (name, what) = perk_upgrade(perk, next);
                (name, what, next)
            }
        }
    }

    /// The weapons' numbers with this run's upgrades.
    pub fn weapons_tuning(&self, base: &WeaponsTuning) -> WeaponsTuning {
        let mut t = base.clone();
        if self.has(WeaponKind::Machete, 1) {
            t.machete.damage *= 1.6;
            t.machete.reach += 40.0;
            t.machete.arc_deg = t.machete.arc_deg.max(110.0);
        }
        if self.has(WeaponKind::Revolver, 1) {
            t.revolver.automatic = true;
            t.revolver.interval *= 0.55;
        }
        if self.has(WeaponKind::Shotgun, 1) {
            t.shotgun.magazine *= 2;
        }
        if self.has(WeaponKind::Shotgun, 2) {
            t.shotgun.damage *= 1.25;
        }
        if self.has(WeaponKind::TommyGun, 1) {
            t.tommy_gun.magazine *= 2;
        }
        if self.has(WeaponKind::TommyGun, 2) {
            t.tommy_gun.damage *= 1.2;
        }
        t
    }

    /// The perk's numbers with this run's upgrades. Only the run's own perk's
    /// numbers matter.
    pub fn perk_tuning(&self, base: &HeartTuning) -> HeartTuning {
        let mut t = base.clone();
        if self.perk >= 1 {
            t.bloodlust_heal *= 1.75;
            t.pulse_radius *= 1.5;
            t.pulse_damage *= 1.75;
            t.second_heart_health = 1.0;
        }
        if self.perk >= 2 {
            t.pulse_cooldown *= 0.45;
        }
        t
    }

    /// How many times the Second Heart can bring you back in a level.
    pub fn revives(&self) -> u32 {
        if self.perk_for == Some(Perk::SecondHeart) && self.perk >= 2 {
            2
        } else {
            1
        }
    }
}

/// A weapon's upgrade at `level` (1 or 2): its name and what it does.
pub fn weapon_upgrade(kind: WeaponKind, level: u8) -> (&'static str, &'static str) {
    match (kind, level.max(1)) {
        (WeaponKind::Machete, 1) => (
            "CLEAVER",
            "A heavier blade. Hits much harder and reaches further, in a wider arc.",
        ),
        (WeaponKind::Machete, _) => (
            "BLOODLETTER",
            "Every hit heals you and puts a little blood back in your heart.",
        ),
        (WeaponKind::Revolver, 1) => (
            "FAN THE HAMMER",
            "Hold the trigger to empty it, nearly twice as fast.",
        ),
        (WeaponKind::Revolver, _) => (
            "RICOCHET",
            "A bullet that hits someone bounces on to the next one nearby.",
        ),
        (WeaponKind::Shotgun, 1) => ("QUAD BARREL", "Four barrels: four shots before a reload."),
        (WeaponKind::Shotgun, _) => (
            "BOOMSTICK",
            "Harder-hitting shells, and every shot throws you backwards. Shoot the floor to fly.",
        ),
        (WeaponKind::TommyGun, 1) => ("BIG DRUM", "A 100-round drum."),
        (WeaponKind::TommyGun, _) => (
            "PIERCING ROUNDS",
            "Harder-hitting bullets that go straight through people.",
        ),
        (WeaponKind::Launcher, 1) => (
            "QUAD LAUNCHER",
            "Four barrels: every shot fires four rockets at once.",
        ),
        (WeaponKind::Launcher, _) => (
            "HOMING ROCKETS",
            "Rockets lock on to whoever's in front of them and chase them down.",
        ),
    }
}

/// A perk's upgrade at `level` (1 or 2): its name and what it does.
pub fn perk_upgrade(perk: Perk, level: u8) -> (&'static str, &'static str) {
    match (perk, level.max(1)) {
        (Perk::Bloodlust, 1) => ("FEAST", "Kills heal you nearly twice as much."),
        (Perk::Bloodlust, _) => ("FRENZY", "Kills also put blood back in your heart."),
        (Perk::Pulse, 1) => (
            "SHOCKWAVE",
            "Pulses reach half as far again, and hit much harder.",
        ),
        (Perk::Pulse, _) => ("QUICKENING", "Pulse more than twice as often."),
        (Perk::SecondHeart, 1) => (
            "STRONG HEART",
            "Your second heart brings you back at full health.",
        ),
        (Perk::SecondHeart, _) => ("THIRD HEART", "Come back twice in each level, not once."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weapons() -> WeaponsTuning {
        ron::from_str(include_str!("../assets/weapons.ron")).unwrap()
    }

    #[test]
    fn offers_run_out_as_things_max_out() {
        let mut upgrades = Upgrades::default();
        assert_eq!(upgrades.offers(None).len(), 5);
        assert_eq!(upgrades.offers(Some(Perk::Pulse)).len(), 6);
        assert_eq!(
            upgrades.take(Offer::Weapon(WeaponKind::Launcher)),
            "QUAD LAUNCHER"
        );
        assert_eq!(
            upgrades.take(Offer::Weapon(WeaponKind::Launcher)),
            "HOMING ROCKETS"
        );
        // Maxed out: no more launcher offers, and taking it again changes nothing.
        assert!(
            !upgrades
                .offers(None)
                .contains(&Offer::Weapon(WeaponKind::Launcher))
        );
        upgrades.take(Offer::Weapon(WeaponKind::Launcher));
        assert_eq!(upgrades.level(WeaponKind::Launcher), MAX_LEVEL);

        upgrades.take(Offer::Perk(Perk::Pulse));
        upgrades.take(Offer::Perk(Perk::Pulse));
        assert!(
            !upgrades
                .offers(Some(Perk::Pulse))
                .contains(&Offer::Perk(Perk::Pulse))
        );
        // A different perk starts from nothing.
        assert!(
            upgrades
                .offers(Some(Perk::Bloodlust))
                .contains(&Offer::Perk(Perk::Bloodlust))
        );
        assert_eq!(upgrades.describe(Offer::Perk(Perk::Bloodlust)).2, 1);
    }

    #[test]
    fn weapon_upgrades_change_the_numbers() {
        let base = weapons();
        let mut upgrades = Upgrades::default();
        assert_eq!(
            upgrades.weapons_tuning(&base).shotgun.magazine,
            base.shotgun.magazine
        );
        for kind in WeaponKind::ALL {
            upgrades.take(Offer::Weapon(kind));
        }
        let t = upgrades.weapons_tuning(&base);
        assert_eq!(t.shotgun.magazine, 4);
        assert_eq!(t.tommy_gun.magazine, 100);
        assert!(t.revolver.automatic && t.revolver.interval < base.revolver.interval);
        assert!(t.machete.damage > base.machete.damage);
        // Level 2s that are about behaviour, not numbers, leave these alone.
        assert_eq!(t.launcher.direct_damage, base.launcher.direct_damage);
    }

    #[test]
    fn perk_upgrades_change_the_numbers() {
        let base: HeartTuning = ron::from_str(include_str!("../assets/heart.ron")).unwrap();
        let mut upgrades = Upgrades::default();
        upgrades.take(Offer::Perk(Perk::SecondHeart));
        assert_eq!(upgrades.perk_tuning(&base).second_heart_health, 1.0);
        assert_eq!(upgrades.revives(), 1);
        upgrades.take(Offer::Perk(Perk::SecondHeart));
        assert_eq!(upgrades.revives(), 2);
        let mut pulse = Upgrades::default();
        pulse.take(Offer::Perk(Perk::Pulse));
        pulse.take(Offer::Perk(Perk::Pulse));
        let t = pulse.perk_tuning(&base);
        assert!(t.pulse_cooldown < base.pulse_cooldown && t.pulse_radius > base.pulse_radius);
    }

    #[test]
    fn every_upgrade_has_a_name() {
        for level in 1..=MAX_LEVEL {
            for kind in WeaponKind::ALL {
                let (name, what) = weapon_upgrade(kind, level);
                assert!(!name.is_empty() && !what.is_empty());
            }
            for perk in Perk::ALL {
                let (name, what) = perk_upgrade(perk, level);
                assert!(!name.is_empty() && !what.is_empty());
            }
        }
        assert_ne!(
            weapon_upgrade(WeaponKind::Shotgun, 1).0,
            weapon_upgrade(WeaponKind::Shotgun, 2).0
        );
    }
}
