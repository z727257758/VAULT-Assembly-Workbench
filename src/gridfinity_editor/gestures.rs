use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridView {
    Plan,
    Scene,
}

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragPreview {
    pub id: u32,
    pub cell: [i32; 2],
    pub span: [u32; 2],
    pub valid: bool,
}

impl Editor {
    pub fn drag_preview(&self) -> Option<&DragPreview> {
        self.drag.as_ref()?.preview.as_ref()
    }

    fn inside_grid(at: [f32; 2], grid: [u32; 2]) -> bool {
        (0..2).all(|axis| (0.0..grid[axis] as f32).contains(&at[axis]))
    }

    pub fn hit_bin(layout: &Layout, at: [f32; 2]) -> Option<u32> {
        layout
            .bins
            .iter()
            .find(|bin| {
                (0..2).all(|axis| {
                    let start = bin.cell[axis] as f32;
                    (start..start + bin.span[axis] as f32).contains(&at[axis])
                })
            })
            .map(|bin| bin.id)
    }

    pub(super) fn width_edge(bin: &Bin, at: [f32; 2], margin: f32) -> i8 {
        let margin = margin.clamp(0., 0.25);
        if margin == 0. {
            return 0;
        }
        let [x, y] = bin.cell.map(|cell| cell as f32);
        if !(y - margin * 0.5..=y + bin.span[1] as f32 + margin * 0.5).contains(&at[1]) {
            return 0;
        }
        if (at[0] - x).abs() <= margin {
            -1
        } else if (at[0] - x - bin.span[0] as f32).abs() <= margin {
            1
        } else {
            0
        }
    }

    pub fn begin_grid_drag(
        &mut self,
        view: GridView,
        layout: &Layout,
        s: &Selection,
        selected: &mut Option<u32>,
        at: [f32; 2],
        edge_margin: f32,
    ) {
        if self.active()
            || !gridfinity::grid_for(s, &layout.baseplate)
                .is_ok_and(|grid| Self::inside_grid(at, grid))
        {
            return;
        }
        let selected_edge = layout
            .bins
            .iter()
            .filter(|bin| Some(bin.id) == *selected)
            .find_map(|bin| {
                let edge = Self::width_edge(bin, at, edge_margin);
                (edge != 0).then_some((bin, edge))
            });
        let hit = selected_edge.or_else(|| {
            let id = Self::hit_bin(layout, at)?;
            Some((layout.bins.iter().find(|bin| bin.id == id)?, 0))
        });
        let (bin, gesture) = if let Some((bin, edge)) = hit {
            *selected = Some(bin.id);
            (
                bin.clone(),
                if edge == 0 {
                    Gesture::Move
                } else {
                    Gesture::Resize([edge, 0])
                },
            )
        } else {
            match Self::fresh(layout) {
                Ok(bin) => (bin, Gesture::Draw(at.map(|v| v.floor() as i32))),
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            }
        };
        self.drag = Some(Drag {
            view,
            preview: None,
            base: layout.clone(),
            specification: s.clone(),
            bin,
            start: at,
            gesture,
        });
        self.error = None;
    }

    /// Coordinates are in cells, from the rear-left corner of the baseplate.
    /// Only the view that owns the gesture can change or commit its preview.
    pub fn update_grid_drag(
        &mut self,
        view: GridView,
        layout: &mut Layout,
        s: &Selection,
        selected: &mut Option<u32>,
        at: Option<[f32; 2]>,
        released: bool,
    ) {
        let Some(drag) = &mut self.drag else { return };
        if drag.view != view {
            return;
        }
        if drag.base != *layout
            || drag.specification.width != s.width
            || drag.specification.depth != s.depth
            || drag.specification.height != s.height
        {
            self.drag = None;
            return;
        }
        let grid = gridfinity::grid_for(s, &layout.baseplate);
        let next = if let Some(at) =
            at.filter(|p| grid.as_ref().is_ok_and(|g| Self::inside_grid(*p, *g)))
        {
            let candidate = drag.candidate(at);
            let next = candidate.layout(&drag.base, s);
            drag.preview = Some(DragPreview {
                id: candidate.bin.id,
                cell: candidate.cell,
                span: candidate.bin.span,
                valid: next.is_ok(),
            });
            next
        } else {
            if let Some(preview) = &mut drag.preview {
                preview.valid = false;
            }
            Err("请在底板网格内释放，已保留原布局".into())
        };
        if released {
            match next {
                Ok(next) => {
                    *selected = Some(drag.bin.id);
                    *layout = next;
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            }
            self.drag = None;
        }
    }
}
