//! Lists of the bottom panel (output, validation results, search results, comments).
//!
//! Like the sheet, a list is a fixed header row above a body that scrolls in both directions, and
//! its columns can be resized by dragging the header separators (double click resets). The widths
//! are kept in egui's temporary memory, so they last for the session but are never saved.

use egui::{Id, Rect, Sense, Ui, Vec2, pos2, vec2};
use egui_extras::{Column, TableBuilder, TableRow};

const HEADER_HEIGHT: f32 = 20.0;
const ROW_HEIGHT: f32 = 18.0;
const MIN_WIDTH: f32 = 30.0;

/// A list column: title and default width. The last column grows to fill the view until it is resized.
pub struct ListColumn<'a> {
    pub title: &'a str,
    pub width: f32,
}

pub const fn col(title: &str, width: f32) -> ListColumn<'_> {
    ListColumn { title, width }
}

/// Memory slot of the resized column widths of list `id` (`Vec<Option<f32>>`, `None` = default width).
pub fn widths_id(id: &str) -> Id {
    Id::unique(("list-widths", id))
}

/// Shows a list with `rows` rows; `add_row` fills one row (cells, selection). Returns the scroll offset of the body.
pub fn show(ui: &mut Ui, id: &str, cols: &[ListColumn<'_>], rows: usize, stick_to_bottom: bool, mut add_row: impl FnMut(&mut TableRow<'_, '_>)) -> Vec2 {
    let mem_id = widths_id(id);
    let stored: Vec<Option<f32>> =
        ui.data(|d| d.get_temp::<Vec<Option<f32>>>(mem_id)).filter(|w| w.len() == cols.len()).unwrap_or_else(|| vec![None; cols.len()]);
    let spacing = ui.spacing().item_spacing.x;
    let mut widths: Vec<f32> = cols.iter().zip(&stored).map(|(c, s)| s.unwrap_or(c.width)).collect();
    if let Some(last) = widths.len().checked_sub(1)
        && stored[last].is_none()
    {
        let used: f32 = widths[..last].iter().map(|w| w + spacing).sum();
        let fill = ui.available_width() - used - 2.0 * spacing - ui.spacing().scroll.allocated_width();
        widths[last] = widths[last].max(fill);
    }
    let (header_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_HEIGHT), Sense::hover());
    let out = egui::ScrollArea::both().id_salt(("list", id)).auto_shrink(false).stick_to_bottom(stick_to_bottom).show(ui, |ui| {
        let mut tb = TableBuilder::new(ui).id_salt(("list-body", id)).striped(true).resizable(false).vscroll(false).sense(Sense::click());
        for &w in &widths {
            tb = tb.column(Column::exact(w).clip(true));
        }
        tb.column(Column::remainder()).body(|body| body.rows(ROW_HEIGHT, rows, |mut row| add_row(&mut row)));
    });
    let offset = out.state.clamped_offset();
    if let Some((i, w)) = header(ui, id, header_rect, offset.x, cols, &widths) {
        let mut stored = stored;
        stored[i] = w;
        ui.data_mut(|d| d.insert_temp(mem_id, stored));
    }
    offset
}

/// Draws the header row. Returns a column whose width was dragged (`Some`) or reset by double click (`None`).
fn header(ui: &Ui, id: &str, rect: Rect, offset_x: f32, cols: &[ListColumn<'_>], widths: &[f32]) -> Option<(usize, Option<f32>)> {
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    let stroke = visuals.widgets.noninteractive.bg_stroke;
    let font = egui::TextStyle::Body.resolve(ui.style());
    let spacing = ui.spacing().item_spacing.x;
    painter.rect_filled(rect, 0.0, visuals.faint_bg_color);
    let mut resized = None;
    let mut x = rect.left() - offset_x;
    for (i, (c, &w)) in cols.iter().zip(widths).enumerate() {
        let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height()));
        let galley = painter.layout_no_wrap(c.title.to_string(), font.clone(), visuals.strong_text_color());
        painter.with_clip_rect(cell.intersect(rect)).galley(
            pos2(cell.left() + 2.0, cell.center().y - galley.size().y / 2.0),
            galley,
            visuals.strong_text_color(),
        );
        let sep = cell.right() + spacing / 2.0;
        painter.vline(sep, rect.y_range(), stroke);
        let handle = Rect::from_center_size(pos2(sep, rect.center().y), vec2(8.0, rect.height())).intersect(rect);
        if handle.width() > 0.0 {
            let resp = ui.interact(handle, ui.scope_id().with(("list-col-resize", id, i)), Sense::click_and_drag());
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if resp.double_clicked() {
                resized = Some((i, None));
            } else if resp.dragged() {
                resized = Some((i, Some((w + resp.drag_delta().x).max(MIN_WIDTH))));
            }
        }
        x = sep + spacing / 2.0;
    }
    painter.hline(rect.x_range(), rect.bottom() - 0.5, stroke);
    resized
}
