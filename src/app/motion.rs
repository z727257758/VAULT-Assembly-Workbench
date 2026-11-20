//! Commit accepted motion to the project preference before replacing its derived scene.
use super::*;
use crate::constraints::{AssemblyRules, PartId};

fn store_lid_preference(members: &mut [Member], owner: usize, value: f32) -> Result<(), String> {
    AssemblyRules::validate_lid_angle(value)?;
    let preset = &mut members
        .get_mut(owner)
        .ok_or("Lid member is unavailable")?
        .preset;
    if preset.assembly_kind != AssemblyKind::Sustainer {
        return Err("Selected member has no lid preference".into());
    }
    preset.sustainer_angle = value;
    Ok(())
}

impl App {
    pub(super) fn commit_lid_preference(&mut self, owner: usize, value: f32) -> Result<(), String> {
        if self.members.is_empty() {
            AssemblyRules::validate_lid_angle(value)?;
            if owner != self.active_member || self.assembly_kind != AssemblyKind::Sustainer {
                return Err("Lid member is unavailable".into());
            }
        } else {
            store_lid_preference(&mut self.members, owner, value)?;
        }
        if owner == self.active_member && self.active_drawer.is_none() {
            self.sustainer_angle = value;
        }
        Ok(())
    }

    pub(super) fn apply_joint_motion(
        &mut self,
        child: &PartId,
        value: f32,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let mut candidate = self.scene.clone();
        let mechanism = AssemblyRules::set_joint_value(&mut candidate, child, value)?;
        if let Some(m) = &mechanism {
            self.commit_motion_goal(m, value)?;
        }
        self.scene = candidate;
        self.scene.calibrated = false;
        self.collision_report = None;
        self.cancel_joint_animation(child);
        if mechanism.is_some() && (self.busy || self.preview.has_pending()) {
            // Replace any in-flight request captured before the preference changed.
            self.assembly(ctx);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lid_motion_persists_only_its_owner_and_invalid_values_leave_the_project_unchanged() {
        let preset: Preset = toml::from_str("bom=[]\nassembly_kind='sustainer'\n[selection]\nwidth='S'\ndepth=3\nheight='1F'\nhidden_key=false").unwrap();
        let member = Member {
            name: "box".into(),
            preset,
            placement: Default::default(),
            exterior: Default::default(),
        };
        let mut members = vec![member.clone(), member];
        store_lid_preference(&mut members, 1, 65.).unwrap();
        let text = toml::to_string(&members[1].preset).unwrap();
        let restored: Preset = toml::from_str(&text).unwrap();
        assert_eq!(restored.sustainer_angle, 65.);
        assert_eq!(members[0].preset.sustainer_angle, 0.);
        for value in [-1., 111., f32::NAN, f32::INFINITY] {
            assert!(store_lid_preference(&mut members, 1, value).is_err());
            assert_eq!(toml::to_string(&members[1].preset).unwrap(), text);
        }
        assert!(store_lid_preference(&mut members, 5, 35.).is_err());
        assert_eq!(toml::to_string(&members[1].preset).unwrap(), text);
    }
}
