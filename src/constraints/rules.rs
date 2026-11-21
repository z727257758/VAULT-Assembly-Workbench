//! Canonical assembly and motion policy. Geometry fitting remains in the specialist modules.
use super::kinematics::LatchJoint;
use super::*;
use std::ops::RangeInclusive;
mod latches;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssemblyKind {
    #[default]
    Drawer,
    Sustainer,
}

pub struct AssemblyRules;

impl AssemblyRules {
    /// Application limits / mesh seating estimates, not manual-specified tolerances.
    pub const LID_ANGLE: RangeInclusive<f32> = 0. ..=110.;
    pub const LOCK_ANGLE: RangeInclusive<f32> = -180. ..=180.;
    pub const DRAWER_SLOT_PITCH: f32 = 48.;
    pub const DRAWER_FIRST_HEIGHT: f32 = 47.;
    pub const SUSTAINER_FIRST_LID_RISE: f32 = 14.;
    pub const PULL_LOCK_RESERVE: f32 = 38.;
    pub const PULL_EXTENSION: RangeInclusive<f32> = 0. ..=800.;
    pub const MIN_PULL_ROD_LENGTH: f32 = 61.;
    pub const DRAWER_REAR_RETENTION: f32 = 20.;
    pub const DRAWER_FLOOR_GAP: f32 = 2.;
    pub const DRAWER_REAR_GAP: f32 = 0.7;
    pub const FACE_GAP: f32 = 0.2;
    pub const DRAWER_SLIDE: &str = "Manual step 10: drawer / housing slide";
    pub const LID_SPEED: f32 = 120.;
    pub const DRAWER_SPEED: f32 = 220.;
    pub const LATCH_SECONDS: f32 = 0.25;
    const LID_SLIDE_SPEED: f32 = 480.;
    const LID_SLIDE_GAP: f32 = 12.;

    pub fn validate_drawer_extension(value: f32) -> Result<(), String> {
        if value.is_finite() && (0. ..=1.).contains(&value) {
            Ok(())
        } else {
            Err("Invalid drawer extension".into())
        }
    }

    pub(crate) fn latch_variant(scene: &Scene, member: u32) -> (bool, bool) {
        let own = |p: &&Part| p.pose.instance / crate::scene::MEMBER_STRIDE == member;
        (
            scene
                .parts
                .iter()
                .filter(own)
                .any(|p| p.pose.path.contains("/LOCK OUTER PART 2-4F")),
            scene
                .parts
                .iter()
                .filter(own)
                .any(|p| p.pose.path.contains("/Modules/") && p.pose.path.contains("/XL4/")),
        )
    }

    pub fn mechanism_for_joint(scene: &Scene, child: &PartId) -> Result<Mechanism, String> {
        let r = scene
            .relations
            .iter()
            .find(|r| &r.child == child)
            .ok_or("Motion joint is unavailable")?;
        r.joint.transform()?;
        let member = child.instance / crate::scene::MEMBER_STRIDE;
        let (kind, min, max, value) = match &r.joint {
            Joint::Slider {
                min, max, offset, ..
            } if Self::label(&r.label) == Self::DRAWER_SLIDE => {
                let part = scene
                    .parts
                    .iter()
                    .find(|p| PartId::of(p) == *child)
                    .ok_or("Drawer is unavailable")?;
                let runner_limit = scene
                    .relations
                    .iter()
                    .filter(|r| &r.parent == child)
                    .filter_map(|r| {
                        if let Joint::RunnerGuide {
                            inner_travel,
                            outer_travel,
                            ..
                        } = r.joint
                        {
                            Some(inner_travel + outer_travel)
                        } else {
                            None
                        }
                    })
                    .reduce(f32::min);
                let limit = runner_limit
                    .unwrap_or_else(|| (part.source_size.y - Self::DRAWER_REAR_RETENTION).max(0.));
                (MotionKind::Drawer, 0_f32.max(*min), max.min(limit), *offset)
            }
            Joint::Hinge {
                min, max, angle, ..
            } if Self::label(&r.label) == crate::sustainer::LID => (
                MotionKind::Lid,
                0_f32.max(*min),
                max.min(*Self::LID_ANGLE.end()),
                *angle,
            ),
            _ => return Err("Selected joint has no supported opening motion".into()),
        };
        if min > 0.001
            || max < min
            || !value.is_finite()
            || value < min - 0.001
            || value > max + 0.001
        {
            return Err(
                "Motion joint does not contain a valid closed datum and travel range".into(),
            );
        }
        let (latch_joints, latch) = Self::latch_state(scene, kind, child)?;
        if !latch_joints.is_empty() && value > 0.001 && latch < 0.999 {
            return Err("An open mechanism requires released locks".into());
        }
        let member = member as usize;
        let supports_upper = scene.member_supports.iter().any(|s| s.contains(&member));
        let (slide, slide_distance) = if kind == MotionKind::Lid {
            (
                scene.member_lid_slides.get(member).copied().unwrap_or(0.),
                if supports_upper {
                    Self::lid_slide_distance(scene, member).unwrap_or(0.)
                } else {
                    0.
                },
            )
        } else {
            (0., 0.)
        };
        Ok(Mechanism {
            child: child.clone(),
            kind,
            min,
            max,
            value,
            latch,
            latch_joints,
            supports_upper,
            slide,
            slide_distance,
        })
    }

    /// Separate the preview box from the closed envelopes of components above its seat.
    fn lid_slide_distance(scene: &Scene, member: usize) -> Option<f32> {
        let (_, hi) = scene.member_bounds.get(member)?;
        let mut supported = vec![member];
        let mut next = 0;
        let mut left = f32::INFINITY;
        while let Some(&below) = supported.get(next) {
            for (above, supports) in scene.member_supports.iter().enumerate() {
                if supports.contains(&below) && !supported.contains(&above) {
                    let x = scene.member_bounds.get(above)?.0.x;
                    if !x.is_finite() {
                        return None;
                    }
                    left = left.min(x);
                    supported.push(above);
                }
            }
            next += 1;
        }
        let distance = hi.x - left + Self::LID_SLIDE_GAP;
        (distance.is_finite() && distance > 0.).then_some(distance)
    }

    pub(crate) fn mechanism(
        scene: &Scene,
        target: crate::scene::Target,
    ) -> Result<Mechanism, String> {
        let candidates: Vec<_> = scene
            .relations
            .iter()
            .filter(|r| {
                [Self::DRAWER_SLIDE, crate::sustainer::LID].contains(&Self::label(&r.label))
            })
            .filter(|r| {
                scene
                    .parts
                    .iter()
                    .find(|p| PartId::of(p) == r.child)
                    .is_some_and(|p| target.contains(p))
            })
            .collect();
        match candidates.as_slice() {
            [r] => Self::mechanism_for_joint(scene, &r.child),
            [] => Err("当前对象没有可用的开合关节 / No opening joint for this target".into()),
            _ => Err("请选择共用外壳中的具体抽屉 / Select an individual drawer".into()),
        }
    }

    fn validate_motion_value(m: &Mechanism, value: f32, slide: f32) -> Result<(), String> {
        if !value.is_finite() || !(m.min..=m.max).contains(&value) {
            return Err("Motion target exceeds its joint travel".into());
        }
        let max_slide = m.slide.max(m.slide_distance);
        if !slide.is_finite() || slide < 0. || slide > max_slide * (1. + f32::EPSILON) {
            return Err("Box slide exceeds its preview travel".into());
        }
        if m.blocked_by_upper_at(slide) && value > 0. {
            return Err("支承上层的箱盖必须保持闭合 / Supporting lid must remain closed".into());
        }
        Ok(())
    }

    pub fn plan_motion(scene: &Scene, child: &PartId, goal: f32) -> Result<MotionPlan, String> {
        let m = Self::mechanism_for_joint(scene, child)?;
        let goal_slide = if goal > m.min {
            m.slide.max(m.slide_distance)
        } else {
            0.
        };
        Self::validate_motion_value(&m, goal, goal_slide)?;
        let slide_out_time = (goal_slide - m.slide).max(0.) / Self::LID_SLIDE_SPEED;
        let slide_in_time = (m.slide - goal_slide).max(0.) / Self::LID_SLIDE_SPEED;
        let release_time = if m.has_latch() && goal > m.min + 0.001 {
            (1. - m.latch) * Self::LATCH_SECONDS
        } else {
            0.
        };
        let move_time = (goal - m.value).abs()
            / if m.kind == MotionKind::Lid {
                Self::LID_SPEED
            } else {
                Self::DRAWER_SPEED
            };
        let lock_time = if m.has_latch() && goal <= m.min + 0.001 {
            (if move_time > 0. { 1. } else { m.latch }) * Self::LATCH_SECONDS
        } else {
            0.
        };
        let plan = MotionPlan {
            mechanism: m,
            goal,
            duration: slide_out_time + release_time + move_time + lock_time + slide_in_time,
            latch_only: None,
            goal_slide,
            slide_out_time,
            slide_in_time,
            release_time,
            move_time,
            lock_time,
        };
        Self::solve_sample(
            &mut scene.clone(),
            &plan.mechanism,
            plan.sample(plan.duration),
        )?;
        Ok(plan)
    }

    /// Apply a preview path sample without rebuilding the member meshes.
    pub fn apply_motion_sample(
        scene: &mut Scene,
        child: &PartId,
        sample: MotionSample,
    ) -> Result<(), String> {
        let m = Self::mechanism_for_joint(scene, child)?;
        Self::transaction(scene, |candidate| Self::solve_sample(candidate, &m, sample))
    }

    fn solve_sample(scene: &mut Scene, m: &Mechanism, sample: MotionSample) -> Result<(), String> {
        Self::set_motion_sample(scene, m, sample)?;
        solve(scene)
    }

    /// Build an editing pose without changing the assembly or saved motion goals.
    pub(crate) fn interior_preview(source: &Scene, target: &Mechanism) -> Result<Scene, String> {
        let mut scene = source.clone();
        for relation in &source.relations {
            if relation.child == target.child
                || ![Self::DRAWER_SLIDE, crate::sustainer::LID]
                    .contains(&Self::label(&relation.label))
            {
                continue;
            }
            let mechanism = Self::mechanism_for_joint(source, &relation.child)?;
            if mechanism.value != mechanism.min || mechanism.latch != 0. || mechanism.slide != 0. {
                Self::set_motion_sample(
                    &mut scene,
                    &mechanism,
                    MotionSample {
                        value: mechanism.min,
                        latch: 0.,
                        slide: 0.,
                    },
                )?;
            }
        }
        Self::set_motion_sample(
            &mut scene,
            target,
            MotionSample {
                value: target.max,
                latch: if target.has_latch() && target.max > target.min + 0.001 {
                    1.
                } else {
                    0.
                },
                slide: target.slide.max(target.slide_distance),
            },
        )?;
        solve(&mut scene)?;
        Ok(scene)
    }

    fn set_motion_sample(
        scene: &mut Scene,
        m: &Mechanism,
        sample: MotionSample,
    ) -> Result<(), String> {
        Self::validate_motion_value(m, sample.value, sample.slide)?;
        if !sample.latch.is_finite() || !(0. ..=1.).contains(&sample.latch) {
            return Err("Invalid motion sample".into());
        }
        if !m.has_latch() && sample.latch > 0. {
            return Err("This mechanism has no lock to release".into());
        }
        if m.has_latch() && sample.value > 0.001 && sample.latch < 0.999 {
            return Err("Release the lock before moving the drawer or lid".into());
        }
        if m.blocked_by_upper_at(sample.slide)
            && (sample.latch > m.latch + 1e-6 || sample.slide < m.slide && sample.latch > 1e-6)
        {
            return Err("A supporting lid cannot be unlocked".into());
        }
        if sample.slide != m.slide {
            let member = m.child.instance / crate::scene::MEMBER_STRIDE;
            let shift = m.slide - sample.slide;
            for part in &mut scene.parts {
                if part.pose.instance / crate::scene::MEMBER_STRIDE == member {
                    part.pose.position[0] += shift;
                }
            }
            let member = member as usize;
            if scene.member_lid_slides.len() <= member {
                scene.member_lid_slides.resize(member + 1, 0.);
            }
            scene.member_lid_slides[member] = sample.slide;
        }
        let driver = scene
            .relations
            .iter_mut()
            .find(|r| r.child == m.child)
            .ok_or("Motion joint changed")?;
        match &mut driver.joint {
            Joint::Slider { offset, .. } => *offset = sample.value,
            Joint::Hinge { angle, .. } => *angle = sample.value,
            _ => return Err("Motion joint changed".into()),
        }
        for latch in &m.latch_joints {
            let value = match &mut scene.relations[latch.relation].joint {
                Joint::Slider { offset, .. } => offset,
                Joint::Hinge { angle, .. } => angle,
                _ => return Err("Lock joint changed".into()),
            };
            *value = latch.released * sample.latch;
        }
        Ok(())
    }

    pub fn close_drawers(scene: &mut Scene) -> Result<(), String> {
        let children: Vec<_> = scene
            .relations
            .iter()
            .filter(|r| Self::label(&r.label) == Self::DRAWER_SLIDE)
            .map(|r| r.child.clone())
            .collect();
        for child in children {
            Self::set_slider(scene, &child, 0.)?;
        }
        Ok(())
    }

    /// Nominal installation height for layout prechecks, not a measured mesh envelope.
    pub fn installation_height(height: &str, is_box: bool) -> Result<f32, String> {
        let units = tower::units(height)?;
        Ok(Self::DRAWER_FIRST_HEIGHT
            + (units - 1) as f32 * Self::DRAWER_SLOT_PITCH
            + if is_box && units == 1 {
                Self::SUSTAINER_FIRST_LID_RISE
            } else {
                0.
            })
    }

    /// Uses the same validated stack heights as the support check.
    pub fn pull_limit(
        modules: &[stacking::Module<'_>],
        layout: &stacking::Layout,
        index: usize,
    ) -> f32 {
        (layout.nominal_tops[index]
            - if modules[index].is_box && modules[index].selection.height == "1F" {
                Self::SUSTAINER_FIRST_LID_RISE
            } else {
                0.
            }
            - Self::PULL_LOCK_RESERVE)
            .clamp(*Self::PULL_EXTENSION.start(), *Self::PULL_EXTENSION.end())
    }

    pub fn family(scene: &Scene) -> AssemblyKind {
        if scene.parts.iter().any(|p| {
            p.pose.path.starts_with(crate::sustainer::PREFIX) && p.pose.path.contains("/Modules/")
        }) {
            AssemblyKind::Sustainer
        } else {
            AssemblyKind::Drawer
        }
    }

    /// Default load policy retains unsupported drawer variants as illustrative geometry.
    /// Strict graph construction is available through `assemble_drawer`.
    pub fn initialize(scene: &mut Scene, items: &[crate::catalog::Bom]) -> Result<(), String> {
        Self::transaction(scene, |candidate| {
            Self::seed_scene(candidate, items);
            match Self::family(candidate) {
                AssemblyKind::Sustainer => crate::sustainer::apply(candidate),
                AssemblyKind::Drawer => {
                    if let Err(error) = Self::assemble_drawer(candidate) {
                        candidate.calibrated = false;
                        candidate.constraint_warning = Some(error);
                    }
                    Ok(())
                }
            }
        })
    }

    /// Illustrative starting poses and palette; exact mates refine these on the worker.
    fn seed_scene(scene: &mut Scene, items: &[crate::catalog::Bom]) {
        let size = scene
            .parts
            .iter()
            .find(|p| {
                p.pose
                    .path
                    .rsplit('/')
                    .next()
                    .is_some_and(|n| n.contains(" Module "))
            })
            .map(|p| p.source_size)
            .unwrap_or(Vec3::new(160., 60., 150.));
        let (w, h, d) = (size.x, size.y, size.z);
        let mut indices = std::collections::HashMap::<String, usize>::new();
        for part in &mut scene.parts {
            let index = indices.entry(part.pose.path.clone()).or_default();
            let display_index = *index;
            *index += 1;
            let side = if display_index.is_multiple_of(2) {
                -1.
            } else {
                1.
            };
            let n = part.pose.path.rsplit('/').next().unwrap_or("");
            let group = items
                .iter()
                .find(|b| b.path == part.pose.path)
                .map_or("", |b| b.group.as_str());
            let mut pos = Vec3::ZERO;
            let mut rot = Vec3::ZERO;
            let mut travel = Vec3::ZERO;
            let mut color = [0.65, 0.58, 0.85, 1.];
            if n.contains(" Module ") {
                rot.x = 90.;
                travel = Vec3::new(0., h * 0.9, -d * 0.5);
                color = [0.33, 0.54, 0.72, 1.];
            } else if n.contains("Box") || n.contains("Back Panel") {
                pos.y = -h * 0.1;
                travel.z = d * 0.35;
                color = [0.45, 0.70, 0.60, 1.];
                if n.contains("Part 1") {
                    pos.z = -d * 0.22;
                }
                if n.contains("Part 2") || n.contains("Back Panel") {
                    pos.z = d * 0.22;
                    travel.z += d * 0.3;
                }
            } else if n.contains("Roof") {
                rot = Vec3::new(90., -45., 0.);
                pos.y = h * 0.5 + 6.;
                travel.y = h * 2.;
                color = [0.80, 0.84, 0.88, 1.];
            } else if n.contains("Runner Guide") || n.contains("Runner Slide") {
                rot.z = side * 90.;
                pos = Vec3::new(side * (w * 0.5 - 9.), -h * 0.2, 0.);
                travel = Vec3::new(
                    side * w * if n.contains("Guide") { 0.65 } else { 0.35 },
                    0.,
                    d * 0.25,
                );
                color = if n.contains("Guide") {
                    [0.78, 0.82, 0.88, 1.]
                } else {
                    [0.30, 0.68, 0.73, 1.]
                };
            } else if n.contains("Rails") {
                rot.x = 90.;
                pos = Vec3::new(0., side * h * 0.22, d * 0.46);
                travel = Vec3::new(0., side * h * 0.9, d * 0.65);
                color = [0.87, 0.57, 0.28, 1.];
            } else if part.pose.path.contains("/Facades/") {
                rot.x = 90.;
                pos.z = d * 0.52;
                travel.z = d * 0.9;
                color = [0.80, 0.84, 0.88, 1.];
            } else if n.contains("Stopper") {
                pos = Vec3::new(side * (w * 0.5 - 8.), -h * 0.3, d * 0.47);
                travel = Vec3::new(side * w * 0.85, -h * 0.5, d * 0.8);
                color = [0.88, 0.60, 0.32, 1.];
            } else if n.contains("Springs") {
                rot.x = 90.;
                pos = Vec3::new(side * w * 0.24, 0., d * 0.53);
                travel = Vec3::new(side * w * 0.45, h * 0.8, d * 1.1);
                color = [0.88, 0.60, 0.32, 1.];
            } else if n.contains("Latch") {
                pos.y = h * 0.5;
                travel = Vec3::new(0., h * 1.5, -d * 0.5);
                color = [0.88, 0.60, 0.32, 1.];
            } else if group == "Key" || group == "Mechanism" {
                rot.x = 90.;
                let slot = if n.contains("Label") {
                    0.
                } else if n.contains("Facade Cap") {
                    1.
                } else if n.contains("Key Cap") {
                    2.
                } else if n.contains("Part 2") {
                    4.
                } else {
                    3.
                };
                pos = Vec3::new((slot - 1.5) * w * 0.08, 0., d * 0.55 + slot * 3.);
                travel = Vec3::new((slot - 1.5) * w * 0.28, -h * 0.8, d * (1.1 + slot * 0.12));
            } else {
                pos = Vec3::new(0., -h * 0.25, 0.);
                travel = Vec3::new(w * (part.pose.instance as f32 + 1.), -h, 0.);
            }

            part.pose.position = pos.to_array();
            part.pose.rotation = rot.to_array();
            part.pose.travel = travel.to_array();
            part.pose.travel_frame = None;
            part.color = color;
        }
    }

    pub fn layout(
        modules: &[stacking::Module<'_>],
        configs: &[crate::catalog::exterior::Exterior],
        gap: f32,
    ) -> Result<stacking::Layout, String> {
        if modules.len() != configs.len() {
            return Err("Exterior configuration count differs from module count".into());
        }
        let layout = stacking::plan(modules, gap)?;
        exterior::validate_layout(modules, &layout, configs)?;
        Ok(layout)
    }

    /// Seat supporting boxes with their lids closed before applying any preview slide.
    pub fn preview_lid_angle(layout: &stacking::Layout, member: usize, requested: f32) -> f32 {
        if layout.supports.iter().any(|below| below.contains(&member)) {
            0.
        } else {
            requested
        }
    }

    pub fn shared_shell(shell: Scene, drawers: Vec<(Scene, u8)>) -> Result<Scene, String> {
        crate::scene::drawers::compose(shell, drawers)
    }

    pub fn drawer_mounts(scene: &Scene) -> Result<Vec<SliderMount>, String> {
        scene
            .relations
            .iter()
            .filter(|r| Self::label(&r.label) == Self::DRAWER_SLIDE)
            .map(|r| {
                r.joint.transform()?;
                let Joint::Slider {
                    axis,
                    offset,
                    min,
                    max,
                    ..
                } = r.joint
                else {
                    return Err("Drawer motion joint is not a slider".into());
                };
                let parent = scene
                    .parts
                    .iter()
                    .find(|p| PartId::of(p) == r.parent)
                    .ok_or("Drawer parent is unavailable")?;
                Ok(SliderMount {
                    child: r.child.clone(),
                    world_axis: parent
                        .pose
                        .matrix(0.)
                        .transform_vector3(axis.into())
                        .normalize(),
                    offset,
                    min,
                    max,
                })
            })
            .collect()
    }

    pub fn bind_drawer_roots(
        scene: &mut Scene,
        parent: &PartId,
        parent_inverse: Mat4,
        drawer: usize,
        mounts: &[SliderMount],
    ) -> Result<(), String> {
        let children: HashSet<_> = scene.relations.iter().map(|r| r.child.clone()).collect();
        for part in &scene.parts {
            let id = PartId::of(part);
            if children.contains(&id) {
                continue;
            }
            let (_, rotation, position) =
                (parent_inverse * part.pose.matrix(0.)).to_scale_rotation_translation();
            let (joint, label) = if let Some(m) = mounts.iter().find(|m| m.child == id) {
                let axis = parent_inverse.transform_vector3(m.world_axis).normalize();
                (
                    Joint::Slider {
                        position: (position - axis * m.offset).to_array(),
                        rotation: rotation.to_array(),
                        axis: axis.to_array(),
                        offset: m.offset,
                        min: m.min,
                        max: m.max,
                    },
                    Self::DRAWER_SLIDE.into(),
                )
            } else {
                (
                    Joint::Fixed {
                        position: position.to_array(),
                        rotation: rotation.to_array(),
                    },
                    format!("Drawer slot {} (estimated)", drawer + 1),
                )
            };
            joint.transform()?;
            scene.relations.push(Relation {
                parent: parent.clone(),
                child: id,
                joint,
                label,
            });
        }
        Ok(())
    }

    /// Transaction boundary for graph, solved poses, and derived explosion offsets together.
    fn transaction(
        scene: &mut Scene,
        operation: impl FnOnce(&mut Scene) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut candidate = scene.clone();
        operation(&mut candidate)?;
        *scene = candidate;
        Ok(())
    }

    pub(crate) fn label(label: &str) -> &str {
        label
            .strip_prefix("Member ")
            .and_then(|text| text.split_once(": "))
            .filter(|(index, _)| index.parse::<usize>().is_ok())
            .map_or(label, |(_, label)| label)
    }

    pub fn validate_lid_angle(value: f32) -> Result<(), String> {
        if !value.is_finite() || !Self::LID_ANGLE.contains(&value) {
            Err("Invalid lid angle".into())
        } else {
            Ok(())
        }
    }

    pub fn lid_angle_for(scene: &Scene, child: &PartId) -> Option<f32> {
        let relation = scene
            .relations
            .iter()
            .find(|r| &r.child == child && Self::label(&r.label) == crate::sustainer::LID)?;
        match relation.joint {
            Joint::Hinge { angle, .. } => Some(angle),
            _ => None,
        }
    }

    pub fn lid_angle(scene: &Scene) -> Option<f32> {
        let mut lids = scene
            .relations
            .iter()
            .filter(|r| Self::label(&r.label) == crate::sustainer::LID);
        let lid = lids.next()?;
        if lids.next().is_some() {
            return None;
        }
        match lid.joint {
            Joint::Hinge { angle, .. } => Some(angle),
            _ => None,
        }
    }

    pub fn set_lid_angle(scene: &mut Scene, value: f32) -> Result<(), String> {
        let lids: Vec<_> = scene
            .relations
            .iter()
            .filter(|r| Self::label(&r.label) == crate::sustainer::LID)
            .map(|r| r.child.clone())
            .collect();
        match lids.as_slice() {
            [child] => Self::set_lid_angle_for(scene, child, value),
            [] => Err("Sustainer lid is unavailable".into()),
            _ => Err("Multiple lids: select a lid instance explicitly".into()),
        }
    }

    /// Change one lid, releasing only that member's inner/outer locks before lifting.
    pub fn set_lid_angle_for(scene: &mut Scene, child: &PartId, value: f32) -> Result<(), String> {
        Self::set_motion_endpoint(scene, child, MotionKind::Lid, value).map(|_| ())
    }

    /// Mechanical drawer movement uses the graph's local slider axis and limits.
    /// It never uses the presentation-only `Pose::travel` displacement.
    pub fn set_slider(scene: &mut Scene, child: &PartId, value: f32) -> Result<(), String> {
        Self::set_motion_endpoint(scene, child, MotionKind::Drawer, value).map(|_| ())
    }

    fn set_motion_endpoint(
        scene: &mut Scene,
        child: &PartId,
        kind: MotionKind,
        value: f32,
    ) -> Result<Mechanism, String> {
        let mut mechanism = Self::mechanism_for_joint(scene, child)?;
        if mechanism.kind != kind {
            return Err("Selected joint does not support the requested motion".into());
        }
        let latch = if mechanism.has_latch() && value > mechanism.min + 0.001 {
            1.
        } else {
            0.
        };
        let slide = if value > mechanism.min {
            mechanism.slide
        } else {
            0.
        };
        Self::transaction(scene, |candidate| {
            Self::solve_sample(
                candidate,
                &mechanism,
                MotionSample {
                    value,
                    latch,
                    slide,
                },
            )?;
            if kind == MotionKind::Lid {
                Self::reset_explosion(candidate)?;
            }
            Ok(())
        })?;
        mechanism.value = value;
        mechanism.latch = latch;
        mechanism.slide = slide;
        Ok(mechanism)
    }

    /// One motion entry point for both normal controls and the relation inspector.
    pub fn set_joint_value(
        scene: &mut Scene,
        child: &PartId,
        value: f32,
    ) -> Result<Option<Mechanism>, String> {
        let relation = scene
            .relations
            .iter()
            .find(|r| &r.child == child)
            .ok_or("Joint is unavailable")?;
        if Self::label(&relation.label) == crate::sustainer::LID {
            return Self::set_motion_endpoint(scene, child, MotionKind::Lid, value).map(Some);
        }
        if Self::label(&relation.label) == Self::DRAWER_SLIDE {
            return Self::set_motion_endpoint(scene, child, MotionKind::Drawer, value).map(Some);
        }
        if let Some(owner) = Self::latch_owner(scene, child) {
            let m = Self::mechanism_for_joint(scene, &owner)?;
            let latch = m
                .latch_joints
                .iter()
                .find(|latch| &scene.relations[latch.relation].child == child)
                .ok_or("Lock control is unavailable")?;
            if latch.released.abs() < 0.001 {
                return Err("This lock joint has no independent travel".into());
            }
            let sample = MotionSample {
                value: m.value,
                latch: value / latch.released,
                slide: m.slide,
            };
            return Self::transaction(scene, |candidate| Self::solve_sample(candidate, &m, sample))
                .map(|()| None);
        }
        Self::transaction(scene, |candidate| {
            let relation = candidate
                .relations
                .iter_mut()
                .find(|r| &r.child == child)
                .ok_or("Joint is unavailable")?;
            match &mut relation.joint {
                Joint::Slider { offset, .. } => *offset = value,
                Joint::Hinge { angle, .. } => *angle = value,
                _ => return Err("Joint has no motion value".into()),
            }
            solve(candidate)
        })
        .map(|()| None)
    }

    /// Restore extraction axes per assembly, then separate combination members for display.
    pub fn reset_explosion(scene: &mut Scene) -> Result<(), String> {
        Self::transaction(scene, |candidate| {
            if candidate.member_placements.is_empty() {
                return Self::layout_explosion(candidate);
            }
            if candidate.relations.iter().any(|r| {
                r.parent.instance / crate::scene::MEMBER_STRIDE
                    != r.child.instance / crate::scene::MEMBER_STRIDE
            }) {
                return Err("Cross-member joints require an explicit explosion group".into());
            }
            for member in 0..candidate.member_placements.len() {
                let owns = |id: &PartId| id.instance / crate::scene::MEMBER_STRIDE == member as u32;
                let mut local = Scene {
                    parts: candidate
                        .parts
                        .iter()
                        .filter(|p| owns(&PartId::of(p)))
                        .cloned()
                        .collect(),
                    relations: candidate
                        .relations
                        .iter()
                        .filter(|r| owns(&r.child))
                        .cloned()
                        .collect(),
                    mating_checks: candidate
                        .mating_checks
                        .iter()
                        .filter(|c| owns(&c.child))
                        .cloned()
                        .collect(),
                    assembly: true,
                    ..Default::default()
                };
                for relation in &mut local.relations {
                    relation.label = Self::label(&relation.label).into();
                }
                for check in &mut local.mating_checks {
                    check.label = Self::label(&check.label).into();
                    if let Some(label) = &mut check.when_hinge_closed {
                        *label = Self::label(label).into();
                    }
                }
                Self::layout_explosion(&mut local)?;
                for part in &mut candidate.parts {
                    if let Some(updated) = local
                        .parts
                        .iter()
                        .find(|p| PartId::of(p) == PartId::of(part))
                    {
                        part.pose.travel = updated.pose.travel;
                        part.pose.travel_frame = updated.pose.travel_frame;
                    }
                }
            }
            Self::space_combination(candidate)
        })
    }

    fn layout_explosion(scene: &mut Scene) -> Result<(), String> {
        match Self::family(scene) {
            AssemblyKind::Drawer => crate::explosion::layout_drawer(scene),
            AssemblyKind::Sustainer => crate::sustainer::layout_explosion(scene),
        }
    }

    pub fn space_combination(scene: &mut Scene) -> Result<(), String> {
        crate::explosion::space_combination(scene, &scene.member_placements.clone())
    }

    fn latch_recipes() -> &'static LatchRecipes {
        static RECIPES: std::sync::LazyLock<LatchRecipes> = std::sync::LazyLock::new(|| {
            toml::from_str(include_str!("../../config/sustainer-locks.toml"))
                .expect("Valid latch display calibration")
        });
        &RECIPES
    }
    pub fn latch_display_pose(tall: bool, xl4: bool) -> LatchDisplayPose {
        let recipes = Self::latch_recipes();
        if xl4 {
            recipes.xl4
        } else if tall {
            recipes.tall
        } else {
            recipes.one_floor
        }
    }
    pub fn latch_release_pose() -> LatchDisplayPose {
        let r = Self::latch_recipes();
        LatchDisplayPose {
            inner_degrees: r.release_inner_degrees,
            outer_degrees: r.release_outer_degrees,
        }
    }

    pub fn assemble_drawer(scene: &mut Scene) -> Result<usize, String> {
        if !scene.relations.is_empty() {
            return Err("Basic relations require an empty constraint graph".into());
        }
        let original = scene.clone();
        let result = (|| {
            let module = scene
                .parts
                .iter()
                .position(|p| p.pose.path.contains("/Modules/"));
            let drawer = scene.parts.iter().position(|p| {
                p.pose.path.contains("/Boxes/")
                    && p.pose.path.contains(" Box ")
                    && !p.pose.path.contains(" Part ")
            });
            let facade = scene
                .parts
                .iter()
                .position(|p| p.pose.path.contains("/Facades/"));
            let (Some(module), Some(drawer), Some(facade)) = (module, drawer, facade) else {
                return Err(
                    "Basic relations currently support non-split drawers with a separate facade"
                        .into(),
                );
            };
            crate::module_assembly::prepare(scene, module, drawer, facade)?;
            let mut slider = Relation::fixed(scene, module, drawer, true)?;
            if let Joint::Slider { min, max, .. } = &mut slider.joint {
                *min = 0.;
                *max = (scene.parts[drawer].source_size.y - Self::DRAWER_REAR_RETENTION).max(0.);
            }
            slider.label = Self::DRAWER_SLIDE.into();
            add(scene, slider)?;
            // Attach subassemblies before moving the facade, so descendants follow
            // its correction and subsequent drawer travel rather than staying behind.
            for i in 0..scene.parts.len() {
                if [module, drawer, facade].contains(&i) {
                    continue;
                }
                let path = &scene.parts[i].pose.path;
                let name = path.rsplit('/').next().unwrap_or(path);
                let parent = if name.contains("Runner Guide")
                    || name.contains("Stopper")
                    || name == "Latch.stl"
                    || path.contains("/Roofs/")
                {
                    Some(module)
                } else if name.contains("Runner Slide")
                    || name.contains("Rails")
                    || name.contains("Baseplate")
                {
                    Some(drawer)
                } else if name.contains("Key") || name == "Label.stl" || name == "Facade Cap.stl" {
                    Some(facade)
                } else {
                    None
                };
                if let Some(parent) = parent {
                    let mut relation = Relation::fixed(scene, parent, i, false)?;
                    relation.label = "Subassembly attachment (verify)".into();
                    add(scene, relation)?;
                }
            }
            let (_, box_hi) = world_bounds(&scene.parts[drawer]);
            let (face_lo, face_hi) = world_bounds(&scene.parts[facade]);
            let face_center = (face_lo + face_hi) * 0.5;
            let anchor_a = world_anchor(
                scene,
                drawer,
                Vec3::new(face_center.x, face_center.y, box_hi.z),
                Vec3::Z,
            );
            let anchor_b = world_anchor(
                scene,
                facade,
                Vec3::new(face_center.x, face_center.y, face_lo.z),
                -Vec3::Z,
            );
            let mut mate = Relation::face(scene, anchor_a, anchor_b, Self::FACE_GAP, 0.)?;
            mate.label = "Facade / drawer exterior datum (verify)".into();
            add(scene, mate)?;
            // Planar support datums: spring front against facade rear. Split bodies keep
            // separate IDs and independent anchors. These are starting points, not CAD fits.
            for i in 0..scene.parts.len() {
                if scene.parts[i].pose.path.ends_with("/Springs.stl") {
                    let (lo, hi) = world_bounds(&scene.parts[i]);
                    let center = (lo + hi) * 0.5;
                    let (facade_lo, _) = world_bounds(&scene.parts[facade]);
                    let parent = world_anchor(
                        scene,
                        facade,
                        Vec3::new(center.x, center.y, facade_lo.z),
                        -Vec3::Z,
                    );
                    let child =
                        world_anchor(scene, i, Vec3::new(center.x, center.y, hi.z), Vec3::Z);
                    let mut mate = Relation::face(scene, parent, child, Self::FACE_GAP, 0.)?;
                    mate.label = "Spring / facade rear datum (verify)".into();
                    add(scene, mate)?;
                }
            }
            crate::front_panel::apply(scene, facade)?;
            // Manual front view: label on the left, round screw cap on the right.
            // Rotate the entire front subassembly, retaining all child-local mates.
            rotate_subassembly(scene, facade, Quat::from_rotation_z(std::f32::consts::PI))?;
            crate::module_assembly::latch(scene, module)?;
            crate::runners::apply(scene, module, drawer)?;
            crate::geared_rails::apply(scene, drawer)?;
            crate::baseplate::apply(scene, drawer)?;
            crate::explosion::reset(scene)?;
            Ok(scene.relations.len())
        })();
        if result.is_err() {
            *scene = original;
        }
        result
    }
}
