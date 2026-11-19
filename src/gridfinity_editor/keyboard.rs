use super::*;

#[derive(Clone, Copy)]
enum Action {
    Cycle(bool),
    Move([i32; 2]),
    Resize(usize, i32),
    Duplicate,
    Delete,
}

impl Action {
    fn from_key(key: egui::Key, modifiers: egui::Modifiers) -> Option<Self> {
        use egui::Key;
        if modifiers.ctrl || modifiers.command {
            return (key == Key::D && !modifiers.alt && !modifiers.shift)
                .then_some(Self::Duplicate);
        }
        if modifiers.alt {
            return None;
        }
        Some(match (key, modifiers.shift) {
            (Key::Tab, backwards) => Self::Cycle(backwards),
            (Key::ArrowLeft, true) => Self::Resize(0, -1),
            (Key::ArrowRight, true) => Self::Resize(0, 1),
            (Key::ArrowUp, true) => Self::Resize(1, -1),
            (Key::ArrowDown, true) => Self::Resize(1, 1),
            (Key::ArrowLeft, false) => Self::Move([-1, 0]),
            (Key::ArrowRight, false) => Self::Move([1, 0]),
            (Key::ArrowUp, false) => Self::Move([0, -1]),
            (Key::ArrowDown, false) => Self::Move([0, 1]),
            (Key::Delete, false) => Self::Delete,
            _ => return None,
        })
    }
}

impl Editor {
    pub fn grid_keyboard(
        &mut self,
        response: &egui::Response,
        layout: &mut Layout,
        s: &Selection,
        selected: &mut Option<u32>,
    ) {
        if !response.has_focus() || self.active() {
            return;
        }
        response.ctx.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
        let mut actions = Vec::new();
        response.ctx.input_mut(|input| {
            if input
                .events
                .iter()
                .any(|e| matches!(e, egui::Event::Ime(_)))
            {
                return;
            }
            input.events.retain(|event| {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    repeat,
                    ..
                } = event
                    && let Some(action) = Action::from_key(*key, *modifiers)
                {
                    if !repeat || !matches!(action, Action::Delete | Action::Duplicate) {
                        actions.push(action);
                    }
                    false
                } else {
                    true
                }
            });
        });
        for action in actions {
            self.error = self.apply_key(action, layout, s, selected).err();
        }
    }

    fn apply_key(
        &self,
        action: Action,
        layout: &mut Layout,
        s: &Selection,
        selected: &mut Option<u32>,
    ) -> Result<(), String> {
        let count = layout.bins.len();
        let position = layout.bins.iter().position(|bin| Some(bin.id) == *selected);
        if let Action::Cycle(backwards) = action {
            if count > 0 {
                let next = if backwards {
                    (position.unwrap_or(0) + count - 1) % count
                } else {
                    position.map_or(0, |i| (i + 1) % count)
                };
                *selected = Some(layout.bins[next].id);
            }
            return Ok(());
        }
        let Some(index) = position else { return Ok(()) };
        let bin = layout.bins[index].clone();
        let mut candidate = Candidate {
            cell: bin.cell.map(|v| v as i32),
            bin,
        };
        match action {
            Action::Move(delta) => {
                for (cell, delta) in candidate.cell.iter_mut().zip(delta) {
                    *cell += delta;
                }
            }
            Action::Resize(axis, delta) => {
                candidate.bin.span[axis] =
                    candidate.bin.span[axis].saturating_add_signed(delta).max(1);
            }
            Action::Duplicate => {
                *selected = Some(layout.add(s, Some(candidate.bin))?);
                return Ok(());
            }
            Action::Delete => {
                layout.bins.remove(index);
                *selected = layout
                    .bins
                    .get(index)
                    .or_else(|| layout.bins.last())
                    .map(|b| b.id);
                return Ok(());
            }
            Action::Cycle(_) => unreachable!(),
        }
        *layout = candidate.layout(layout, s)?;
        Ok(())
    }
}
