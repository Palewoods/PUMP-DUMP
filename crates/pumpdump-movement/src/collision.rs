//! What movement needs to know about the world: "if I sweep this capsule along this
//! path, what do I hit first?"
//!
//! [`CollisionWorld`] is the seam between movement and level data. [`StaticWorld`]
//! implements it with `parry3d` shapes for greybox maps and tests. Real map
//! collision (M4) will be another implementation of the same trait.

use glam::{Quat, Vec3};
use parry3d::math::{Pose, Vector};
use parry3d::query::{Ray, ShapeCastOptions, cast_shapes, contact};
use parry3d::shape::{Capsule as ParryCapsule, SharedShape};

/// An upright capsule. `position` arguments are the **bottom** of the capsule (the
/// feet), because that's what gameplay code cares about.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Capsule {
    pub radius: f32,
    /// Total height, feet to top of head. At least `2 * radius`.
    pub height: f32,
}

impl Capsule {
    /// From the feet to the centre of the capsule.
    pub fn center_offset(&self) -> Vec3 {
        Vec3::Y * (self.height * 0.5)
    }

    /// Half the length of the straight middle section.
    pub fn half_segment(&self) -> f32 {
        (self.height * 0.5 - self.radius).max(0.0)
    }
}

/// The first contact of a sweep.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// How far along the requested motion the contact happens, 0..=1.
    pub fraction: f32,
    /// Contact normal, pointing back towards what was cast. For a capsule touching
    /// an edge or corner this is tilted, even if the surfaces either side are flat.
    pub normal: Vec3,
    /// Where the contact is, world space.
    pub point: Vec3,
}

/// Collision queries against the static world.
pub trait CollisionWorld {
    /// Sweep the capsule from `feet` by `motion` and return the first contact.
    ///
    /// Must ignore surfaces the capsule is moving away from (so a capsule resting
    /// on the floor can still jump) and should report a hit at fraction 0 if the
    /// capsule starts inside something while moving deeper into it.
    fn cast_capsule(&self, capsule: &Capsule, feet: Vec3, motion: Vec3) -> Option<Hit>;

    /// Cast a ray. The normal is the face normal of the surface hit, which is how
    /// movement tells "standing on the edge of a flat step" from "on a steep slope".
    fn cast_ray(&self, origin: Vec3, motion: Vec3) -> Option<Hit>;

    /// The smallest move that leaves the capsule at least `margin` away from
    /// everything it overlaps or nearly touches. Zero when it's already clear.
    fn push_out(&self, capsule: &Capsule, feet: Vec3, margin: f32) -> Vec3;
}

/// A fixed set of convex shapes, tested one by one. Fine for greybox maps with a
/// few hundred pieces. Real maps will want a BVH.
#[derive(Clone, Default)]
pub struct StaticWorld {
    colliders: Vec<(Pose, SharedShape)>,
}

impl StaticWorld {
    pub fn new() -> Self {
        Self::default()
    }

    /// A box, `half_extents` along its local axes, rotated by `rotation`.
    pub fn add_box(&mut self, center: Vec3, half_extents: Vec3, rotation: Quat) {
        let shape = SharedShape::cuboid(half_extents.x, half_extents.y, half_extents.z);
        self.colliders.push((pose(center, rotation), shape));
    }

    /// The convex hull of `points` (world space). Used for ramps and wedges.
    /// Returns `false` if the points are degenerate (all on one plane, etc.).
    pub fn add_convex(&mut self, points: &[Vec3]) -> bool {
        let points: Vec<Vector> = points.iter().map(|&p| to_parry(p)).collect();
        let Some(shape) = SharedShape::convex_hull(&points) else {
            return false;
        };
        self.colliders.push((Pose::identity(), shape));
        true
    }

    pub fn len(&self) -> usize {
        self.colliders.len()
    }

    pub fn is_empty(&self) -> bool {
        self.colliders.is_empty()
    }
}

impl CollisionWorld for StaticWorld {
    fn cast_capsule(&self, capsule: &Capsule, feet: Vec3, motion: Vec3) -> Option<Hit> {
        let shape = parry_capsule(capsule);
        let start = pose(feet + capsule.center_offset(), Quat::IDENTITY);
        let options = ShapeCastOptions {
            max_time_of_impact: 1.0,
            // Discard contacts we're sliding along or leaving (see the trait docs).
            stop_at_penetration: false,
            ..Default::default()
        };

        let mut best: Option<Hit> = None;
        for (shape_pose, other) in &self.colliders {
            // `Err` means parry can't cast this shape pair. Every shape we add is
            // convex, so it can't happen; skipping is the safe fallback.
            let Ok(Some(hit)) = cast_shapes(
                &start,
                to_parry(motion),
                &shape,
                shape_pose,
                Vector::ZERO,
                other.as_ref(),
                options,
            ) else {
                continue;
            };
            if best.is_none_or(|b| hit.time_of_impact < b.fraction) {
                // `normal1` is the capsule's own outward normal at the contact, in
                // its local frame. The capsule is never rotated, so that's world
                // space, and the surface normal is its opposite.
                let normal = -from_parry(hit.normal1);
                best = Some(Hit {
                    fraction: hit.time_of_impact,
                    normal: normal.normalize_or_zero(),
                    // `witness2` is in the hit shape's local frame.
                    point: from_parry(*shape_pose * hit.witness2),
                });
            }
        }
        best
    }

    fn cast_ray(&self, origin: Vec3, motion: Vec3) -> Option<Hit> {
        let ray = Ray::new(to_parry(origin), to_parry(motion));
        let mut best: Option<Hit> = None;
        for (shape_pose, other) in &self.colliders {
            // `solid: false` so a ray starting inside a shape reports where it exits
            // rather than a useless hit at 0 with no normal.
            let Some(hit) = other.cast_ray_and_get_normal(shape_pose, &ray, 1.0, false) else {
                continue;
            };
            if best.is_none_or(|b| hit.time_of_impact < b.fraction) {
                best = Some(Hit {
                    fraction: hit.time_of_impact,
                    normal: from_parry(hit.normal),
                    point: origin + motion * hit.time_of_impact,
                });
            }
        }
        best
    }

    fn push_out(&self, capsule: &Capsule, feet: Vec3, margin: f32) -> Vec3 {
        let shape = parry_capsule(capsule);
        let mut offset = Vec3::ZERO;
        // Two passes settle the usual cases: a floor and a wall, or a corner.
        for _ in 0..2 {
            let here = pose(feet + offset + capsule.center_offset(), Quat::IDENTITY);
            for (shape_pose, other) in &self.colliders {
                let Ok(Some(c)) = contact(&here, &shape, shape_pose, other.as_ref(), margin) else {
                    continue;
                };
                if c.dist < margin {
                    // `normal1` points from the capsule into the other shape.
                    offset -= from_parry(c.normal1) * (margin - c.dist);
                }
            }
        }
        offset
    }
}

fn parry_capsule(capsule: &Capsule) -> ParryCapsule {
    ParryCapsule::new_y(capsule.half_segment(), capsule.radius)
}

// Bevy and parry use different glam versions, so their `Vec3`s are different types.
// These two functions are the only place we cross over.
fn to_parry(v: Vec3) -> Vector {
    Vector::new(v.x, v.y, v.z)
}

fn from_parry(v: Vector) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn pose(translation: Vec3, rotation: Quat) -> Pose {
    let r = parry3d::math::Rot3::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w);
    Pose::from_parts(to_parry(translation), r)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: Capsule = Capsule {
        radius: 16.0,
        height: 72.0,
    };

    fn floor() -> StaticWorld {
        let mut world = StaticWorld::new();
        world.add_box(
            Vec3::new(0.0, -8.0, 0.0),
            Vec3::new(1000.0, 8.0, 1000.0),
            Quat::IDENTITY,
        );
        world
    }

    #[test]
    fn falling_onto_the_floor_hits_it() {
        let hit = floor()
            .cast_capsule(&BODY, Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, -20.0, 0.0))
            .unwrap();
        assert!((hit.fraction - 0.5).abs() < 1e-3, "{hit:?}");
        assert!(hit.normal.abs_diff_eq(Vec3::Y, 1e-3), "{hit:?}");
    }

    #[test]
    fn moving_away_or_along_a_surface_is_not_a_hit() {
        let world = floor();
        // Resting exactly on the floor.
        assert_eq!(world.cast_capsule(&BODY, Vec3::ZERO, Vec3::Y * 10.0), None);
        assert_eq!(
            world.cast_capsule(&BODY, Vec3::new(0.0, 0.1, 0.0), Vec3::X * 10.0),
            None
        );
    }

    #[test]
    fn ramps_report_their_slope_normal() {
        let mut world = StaticWorld::new();
        // 45° wedge rising towards -Z.
        let pts = [
            Vec3::new(-100.0, 0.0, 0.0),
            Vec3::new(100.0, 0.0, 0.0),
            Vec3::new(-100.0, 0.0, -100.0),
            Vec3::new(100.0, 0.0, -100.0),
            Vec3::new(-100.0, 100.0, -100.0),
            Vec3::new(100.0, 100.0, -100.0),
        ];
        assert!(world.add_convex(&pts));
        let hit = world
            .cast_capsule(
                &BODY,
                Vec3::new(0.0, 200.0, -50.0),
                Vec3::new(0.0, -300.0, 0.0),
            )
            .unwrap();
        let expected = Vec3::new(0.0, 1.0, 1.0).normalize();
        assert!(hit.normal.abs_diff_eq(expected, 1e-3), "{hit:?}");
    }
}
