use super::*;
use crate::scene::Part;
use glam::{Mat4, Vec3};

// Intersect the source triangles with the clasp center section. The ends of the
// roof extend farther forward than the lip directly under the hook.
fn section(part: &Part, pose: Mat4, x: f32) -> Vec<Vec3> {
    let mut points = Vec::new();
    for triangle in part.mesh.vertices.chunks_exact(3) {
        let point = |i: usize| pose.transform_point3(Vec3::from(triangle[i].position));
        let vertices = [point(0), point(1), point(2)];
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let (a, b) = (vertices[a], vertices[b]);
            if (a.x < x) != (b.x < x) {
                points.push(a.lerp(b, (x - a.x) / (b.x - a.x)));
            }
        }
    }
    points
}

impl App {
    pub(super) fn check_and_frame_s4_clasp(&mut self) -> Result<(), String> {
        let target = Target {
            member: 2,
            drawer: None,
        };
        let part = |suffix: &str| {
            self.scene
                .parts
                .iter()
                .find(|p| target.contains(p) && p.pose.path.ends_with(suffix))
                .ok_or_else(|| format!("Missing S4 2F clasp fixture part: {suffix}"))
        };
        let outer = part("/LOCK OUTER PART 2-4F.stl")?;
        let roof = part("/S4 Roof.stl")?;
        let pivot = |child: &Part| -> Result<Vec3, String> {
            let relation = self
                .scene
                .relations
                .iter()
                .find(|r| r.child == PartId::of(child))
                .ok_or("Missing clasp hinge")?;
            let Joint::Hinge { pivot, .. } = relation.joint else {
                return Err("Clasp link has no pin axis".into());
            };
            let parent = self
                .scene
                .parts
                .iter()
                .find(|p| PartId::of(p) == relation.parent)
                .ok_or("Missing clasp bearing")?;
            Ok(parent.pose.matrix(0.).transform_point3(Vec3::from(pivot)))
        };
        let small = pivot(part("/LOCK INNER PART.stl")?)?;
        let large = pivot(outer)?;
        let plate = outer.pose.matrix(0.);
        let up_plate = plate.transform_vector3(Vec3::X).normalize();
        let link = large - small;
        // The S4 fixture is upright, with its facade facing +Z. Use actual pins,
        // hook and rim instead of recovering the angles from the animation plan.
        let released = self.smoke_stage != 14;
        let correct_links = if released {
            let tilt = 15_f32.to_radians();
            link.z > 14.
                && (-4. ..-3.5).contains(&link.y)
                && up_plate.abs_diff_eq(Vec3::new(0., tilt.cos(), tilt.sin()), 0.005)
        } else {
            link.y < -14.8 && up_plate.abs_diff_eq(Vec3::Y, 0.005)
        };
        if !correct_links {
            return Err(format!(
                "S4 2F clasp has the wrong linkage or plate direction: released={released}, pin spacing={link:?}, plate={up_plate:?}"
            ));
        }
        let roof_closed = self
            .animation
            .smoke
            .closed_poses
            .get(&PartId::of(roof))
            .ok_or("Missing closed S4 roof pose")?;
        // Offset from the symmetry seam so the section crosses triangle faces.
        let plane_x = small.x + 0.31;
        let roof_section = section(roof, *roof_closed, plane_x);
        let rim = roof_section
            .iter()
            .map(|p| p.z)
            .reduce(f32::max)
            .ok_or("Missing roof rim")?;
        let rim_top = roof_section
            .iter()
            .filter(|p| p.z > rim - 1.)
            .map(|p| p.y)
            .reduce(f32::max)
            .ok_or("Missing roof lip")?;
        // The return bend occupies the final 10 mm of the tall plate's local X length.
        let inverse = plate.inverse();
        let tip = section(outer, plate, plane_x)
            .into_iter()
            .filter(|p| inverse.transform_point3(*p).x > outer.mesh.max.x - 10.)
            .min_by(|a, b| a.z.total_cmp(&b.z))
            .ok_or("Missing upper clasp hook")?;
        let gap = tip.z - rim;
        if (released && gap < 5.) || (!released && (gap >= 0. || tip.y < rim_top - 1.)) {
            return Err(format!(
                "S4 2F hook misses the seated roof lip: released={released}, forward gap={gap:.2} mm, height above lip={:.2} mm",
                tip.y - rim_top
            ));
        }
        // Use the same camera for the two seated poses. With the lid lifted, a
        // left-side view projects the outward direction to the right of the frame.
        let length = outer.mesh.size.x;
        let viewport = self
            .inline_edit
            .viewport
            .ok_or("No clasp close-up viewport")?;
        let panel = (viewport.width() * 0.43)
            .clamp(270., 340.)
            .min(viewport.width() - 32.)
            + 48.;
        self.inline_edit.clear_focus_request();
        let (yaw, pitch): (f32, f32) = if self.smoke_stage == 15 {
            (-70., 10.)
        } else {
            (47., 13.)
        };
        self.camera.yaw = yaw.to_radians();
        self.camera.pitch = pitch.to_radians();
        self.camera.center = small + length * (0.30 * Vec3::Y + 0.23 * Vec3::Z);
        self.camera.radius = (length * 0.84)
            .max(length * 0.70 * viewport.height() / (viewport.width() - panel).max(1.));
        self.camera.zoom = 1.;
        self.camera.pan = [-panel / viewport.width(), 0.];
        Ok(())
    }
}
