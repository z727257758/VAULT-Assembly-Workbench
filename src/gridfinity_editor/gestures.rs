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

impl Drag {
    fn preview_at(&mut self, at: [f32; 2], on_board: bool) -> Result<Layout, String> {
        let candidate = self.candidate(at);
        let next = if on_board {
            candidate.layout(&self.base, &self.specification)
        } else {
            Err("请在底板网格内释放，已保留原布局".into())
        };
        self.preview = Some(DragPreview {
            id: candidate.bin.id,
            cell: candidate.cell,
            span: candidate.bin.span,
            valid: next.is_ok(),
        });
        next
    }
}

impl Editor {
    pub fn drag_preview(&self) -> Option<&DragPreview> {
        self.drag.as_ref()?.preview.as_ref()
    }

    pub fn resizing(&self, view: GridView) -> bool {
        self.drag
            .as_ref()
            .is_some_and(|drag| drag.view == view && matches!(drag.gesture, Gesture::Resize(_)))
    }

    fn inside_grid(at: [f32; 2], grid: [u32; 2]) -> bool {
        (0..2).all(|axis| (0.0..grid[axis] as f32).contains(&at[axis]))
    }

    fn on_resize_boundary(at: [f32; 2], grid: [u32; 2]) -> bool {
        // Screen projection can put an outer handle just beyond the exact plate edge.
        const EPSILON: f32 = 0.003;
        (0..2).all(|axis| (-EPSILON..=grid[axis] as f32 + EPSILON).contains(&at[axis]))
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

    pub fn resize_edges(bin: &Bin, at: [f32; 2], margin: f32) -> [i8; 2] {
        let margin = margin.clamp(0., 0.25);
        if margin == 0. {
            return [0, 0];
        }
        let lo = bin.cell.map(|v| v as f32);
        let hi: [f32; 2] = std::array::from_fn(|i| lo[i] + bin.span[i] as f32);
        if (0..2).any(|i| !(lo[i] - margin..=hi[i] + margin).contains(&at[i])) {
            return [0, 0];
        }
        std::array::from_fn(|i| {
            if (at[i] - lo[i]).abs() <= margin {
                -1
            } else if (at[i] - hi[i]).abs() <= margin {
                1
            } else {
                0
            }
        })
    }

    pub fn resize_cursor(edges: [i8; 2]) -> Option<egui::CursorIcon> {
        use egui::CursorIcon;
        match edges {
            [0, 0] => None,
            [0, _] => Some(CursorIcon::ResizeVertical),
            [_, 0] => Some(CursorIcon::ResizeHorizontal),
            [x, y] if x == y => Some(CursorIcon::ResizeNwSe),
            _ => Some(CursorIcon::ResizeNeSw),
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
        if self.active() {
            return;
        }
        let Ok(grid) = gridfinity::grid_for(s, &layout.baseplate) else {
            return;
        };
        let selected_edge = layout
            .bins
            .iter()
            .filter(|bin| Some(bin.id) == *selected)
            .find_map(|bin| {
                let edges = Self::resize_edges(bin, at, edge_margin);
                (edges != [0, 0]).then_some((bin, edges))
            });
        if !(Self::inside_grid(at, grid)
            || selected_edge.is_some() && Self::on_resize_boundary(at, grid))
        {
            return;
        }
        let hit = selected_edge.or_else(|| {
            let id = Self::hit_bin(layout, at)?;
            Some((layout.bins.iter().find(|bin| bin.id == id)?, [0, 0]))
        });
        let (bin, gesture) = if let Some((bin, edges)) = hit {
            *selected = Some(bin.id);
            (
                bin.clone(),
                if edges == [0, 0] {
                    Gesture::Move
                } else {
                    Gesture::Resize(edges)
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
        let mut drag = Drag {
            view,
            preview: None,
            base: layout.clone(),
            specification: s.clone(),
            bin,
            start: at,
            gesture,
        };
        let _ = drag.preview_at(at, true);
        self.drag = Some(drag);
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
        let next = if let Ok(grid) = gridfinity::grid_for(s, &layout.baseplate)
            && let Some(at) = at.filter(|p| p.iter().all(|v| v.is_finite()))
        {
            let resizing = matches!(drag.gesture, Gesture::Resize(_));
            let on_board =
                Self::inside_grid(at, grid) || resizing && Self::on_resize_boundary(at, grid);
            let point = std::array::from_fn(|axis| {
                let end = grid[axis] as f32;
                at[axis].clamp(0., if resizing { end } else { end.next_down() })
            });
            drag.preview_at(point, on_board)
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
