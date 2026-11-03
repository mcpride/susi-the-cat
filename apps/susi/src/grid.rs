//! Central translation sheet.

use egui::{Align, Color32, Key, Layout, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use egui_extras::{Column, TableBuilder};
use susi_core::i18n::{t, trn};
use susi_core::model::{Project, Row, RowRef};
use susi_core::status::{RowStatus, TranslationStatus};

use crate::actions::Action;
use crate::app::{Nav, SusiApp};
use crate::panels::{row_menu_items, status_color};

const ROW_HEIGHT: f32 = 20.0;

#[derive(Default)]
struct Events {
    click: Option<(RowRef, Option<String>)>,
    double: Option<(RowRef, String)>,
    commit: bool,
    next: bool,
    cancel: bool,
    actions: Vec<Action>,
}

struct Palette {
    lavender: Color32,
    problem: Color32,
    comment: Color32,
}

impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self { lavender: Color32::from_rgb(44, 44, 68), problem: Color32::from_rgb(96, 40, 56), comment: Color32::from_rgb(230, 80, 80) }
        } else {
            Self { lavender: Color32::from_rgb(234, 234, 250), problem: Color32::from_rgb(255, 214, 226), comment: Color32::from_rgb(220, 30, 30) }
        }
    }
}

pub fn show(app: &mut SusiApp, ui: &mut Ui) {
    sheet_toolbar(app, ui);
    ui.separator();
    if app.project.is_none() {
        ui.centered_and_justified(|ui| {
            ui.label(t("No project open – File › Open… or File › New project…"));
        });
        return;
    }
    let langs = app.visible_langs();
    let ev = table(app, ui, &langs);
    apply(app, ev);
}

fn sheet_toolbar(app: &mut SusiApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(t("Status:"));
        for s in TranslationStatus::ALL {
            if ui.add(egui::Button::new(RichText::new("■").color(status_color(s))).small()).on_hover_text(s.label()).clicked() {
                app.actions.push(Action::SetStatus(s));
            }
        }
        ui.separator();
        for (t, hint, n) in
            [("⏮", t("First row"), Nav::First), ("◀", t("Previous row"), Nav::Prev), ("▶", t("Next row"), Nav::Next), ("⏭", t("Last row"), Nav::Last)]
        {
            if ui.small_button(t).on_hover_text(hint).clicked() {
                app.actions.push(Action::Nav(n));
            }
        }
        ui.separator();
        if ui.small_button("🔒").on_hover_text(t("Toggle do not translate")).clicked() {
            app.actions.push(Action::ToggleLocked);
        }
        if ui.small_button("★").on_hover_text(t("Toggle mark")).clicked() {
            app.actions.push(Action::ToggleMarked);
        }
        if ui.small_button("✏").on_hover_text(t("Edit in text editor")).clicked() {
            app.show_bottom = true;
            app.tab = crate::app::BottomTab::Editor;
        }
        ui.separator();
        ui.label(trn(app.rows.len(), "{n} row", "{n} rows"));
    });
}

fn one_line(s: &str) -> String {
    if s.contains(['\n', '\r']) { s.replace("\r\n", " ¶ ").replace(['\n', '\r'], " ¶ ") } else { s.to_string() }
}

fn has_newline(s: &str) -> bool {
    s.contains(['\n', '\r'])
}

fn fill_cell(ui: &Ui, color: Color32) {
    ui.painter().rect_filled(ui.max_rect(), 0.0, color);
}

/// Small triangle in the upper right corner marking a comment.
fn comment_marker(ui: &Ui, color: Color32) {
    let r = ui.max_rect();
    let p = r.right_top();
    ui.painter().add(egui::Shape::convex_polygon(vec![p, p + vec2(-7.0, 0.0), p + vec2(0.0, 7.0)], color, Stroke::NONE));
}

fn cell_label(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(one_line(text)).selectable(false).truncate());
}

fn marker_cell(ui: &mut Ui, row: &Row) {
    let (glyph, color) = match row.status() {
        RowStatus::New => ("✚", Color32::from_rgb(30, 150, 60)),
        RowStatus::Changed => ("✏", Color32::from_rgb(230, 140, 20)),
        RowStatus::Unused => ("✖", Color32::GRAY),
        RowStatus::InUse => ("", Color32::GRAY),
    };
    let mut s = glyph.to_string();
    if row.locked() {
        s.push('🔒');
    }
    if row.marked() {
        s.push('★');
    }
    ui.add(egui::Label::new(RichText::new(s).color(color)).selectable(false));
}

fn marker_hint(row: &Row) -> String {
    let mut parts = vec![row.status().label().to_string()];
    if row.locked() {
        parts.push(t("Do not translate").to_string());
    }
    if row.marked() {
        parts.push(t("Marked").to_string());
    }
    if row.excluded() {
        parts.push(t("Excluded").to_string());
    }
    parts.join(", ")
}

fn status_cell(ui: &mut Ui, row: &Row, lang: Option<&str>) {
    let Some(l) = lang else {
        return;
    };
    let s = row.tstatus(l);
    ui.add(egui::Label::new(RichText::new(format!("⏺ {}", s.label())).color(status_color(s))).selectable(false).truncate());
}

struct LangCell<'a> {
    row: &'a Row,
    lang: &'a str,
    original_lang: &'a str,
    problem: bool,
}

fn lang_cell(ui: &mut Ui, c: &LangCell<'_>, pal: &Palette) {
    if c.problem {
        fill_cell(ui, pal.problem);
    }
    let rect = ui.max_rect();
    if c.lang == c.original_lang {
        fill_cell(ui, pal.lavender);
        cell_label(ui, c.row.original());
        return;
    }
    let st = c.row.tstatus(c.lang);
    ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(3.0, rect.height())), 0.0, status_color(st));
    ui.add_space(4.0);
    let t = c.row.translation(c.lang);
    if let Some(t) = t {
        cell_label(ui, &t.text);
        if !t.comment().is_empty() {
            comment_marker(ui, pal.comment);
        }
    }
}

const HEADER_HEIGHT: f32 = 22.0;

/// A column of the sheet. Widths are stored per [`Col::key`] so they survive language switches.
#[derive(Clone, PartialEq)]
enum Col {
    Marker,
    Context,
    Status,
    Original,
    Lang(String),
    Note,
    TComment,
}

impl Col {
    fn key(&self) -> String {
        match self {
            Self::Marker => "marker".into(),
            Self::Context => "context".into(),
            Self::Status => "status".into(),
            Self::Original => "original".into(),
            Self::Lang(l) => format!("lang:{l}"),
            Self::Note => "note".into(),
            Self::TComment => "tcomment".into(),
        }
    }

    fn default_width(&self) -> f32 {
        match self {
            Self::Marker => 34.0,
            Self::Context => 220.0,
            Self::Status => 130.0,
            Self::Original | Self::Lang(_) => 280.0,
            Self::Note | Self::TComment => 170.0,
        }
    }

    fn min_width(&self) -> f32 {
        match self {
            Self::Marker => 34.0,
            Self::Original | Self::Lang(_) | Self::Context => 60.0,
            Self::Status | Self::Note | Self::TComment => 40.0,
        }
    }

    fn title(&self, cur_lang: Option<&str>) -> String {
        match self {
            Self::Marker => String::new(),
            Self::Context => t("Context").to_string(),
            Self::Status => susi_core::tf!("Status {lang}", lang = cur_lang.unwrap_or("")),
            Self::Original => t("Original").to_string(),
            Self::Lang(l) => SusiApp::lang_label(l),
            Self::Note => t("Row comment").to_string(),
            Self::TComment => t("Comment").to_string(),
        }
    }
}

fn columns(app: &SusiApp, langs: &[String]) -> Vec<Col> {
    let mut cols = vec![Col::Marker];
    if app.settings.show_id_column {
        cols.push(Col::Context);
    }
    cols.extend([Col::Status, Col::Original]);
    cols.extend(langs.iter().cloned().map(Col::Lang));
    if app.settings.show_comment_column {
        cols.push(Col::Note);
    }
    if !app.all_langs {
        cols.push(Col::TComment);
    }
    cols
}

/// The sheet: a fixed header row above a body that scrolls in both directions.
///
/// `egui_extras` tables only scroll vertically, so the body table runs without its own scroll
/// area inside a `ScrollArea::both`, and the header is drawn separately with the body's
/// horizontal offset. Column widths are kept in [`SusiApp::col_widths`].
fn table(app: &mut SusiApp, ui: &mut Ui, langs: &[String]) -> Events {
    let mut ev = Events::default();
    let cols = columns(app, langs);
    let widths: Vec<f32> = cols.iter().map(|c| app.col_widths.get(&c.key()).copied().unwrap_or_else(|| c.default_width())).collect();
    let (header_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_HEIGHT), Sense::hover());
    let scroll_to = if std::mem::take(&mut app.scroll_to_selected) { app.selected_index() } else { None };
    let Some(p) = app.project.as_ref() else {
        return ev;
    };
    let ctx = BodyCtx {
        p,
        rows: &app.rows,
        sel_idx: app.selected.as_ref().and_then(|s| app.rows.iter().position(|r| r == s)),
        cur_lang: app.lang.as_deref(),
        cols: &cols,
        widths: &widths,
        pal: Palette::new(ui.visuals().dark_mode),
        issue_cells: &app.issue_cells,
    };
    let cell_edit = &mut app.cell_edit;
    let out = egui::ScrollArea::both().id_salt("grid-body").auto_shrink(false).show(ui, |ui| body(ui, &ctx, scroll_to, cell_edit, &mut ev));
    app.grid_offset = out.state.clamped_offset();
    if let Some((i, w)) = header(ui, header_rect, app.grid_offset.x, &cols, &widths, app.lang.as_deref()) {
        app.col_widths.insert(cols[i].key(), w);
    }
    ev
}

/// Draws the header row; returns a column width changed by dragging (or reset by double click) a separator.
fn header(ui: &mut Ui, rect: Rect, offset_x: f32, cols: &[Col], widths: &[f32], cur_lang: Option<&str>) -> Option<(usize, f32)> {
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals().clone();
    let stroke = visuals.widgets.noninteractive.bg_stroke;
    let font = egui::TextStyle::Body.resolve(ui.style());
    let spacing = ui.spacing().item_spacing.x;
    painter.rect_filled(rect, 0.0, visuals.faint_bg_color);
    let mut resized = None;
    let mut x = rect.left() - offset_x;
    for (i, (col, &w)) in cols.iter().zip(widths).enumerate() {
        let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height()));
        let galley = painter.layout_no_wrap(col.title(cur_lang), font.clone(), visuals.strong_text_color());
        painter.with_clip_rect(cell.intersect(rect)).galley(
            pos2(cell.left() + 2.0, cell.center().y - galley.size().y / 2.0),
            galley,
            visuals.strong_text_color(),
        );
        let sep = cell.right() + spacing / 2.0;
        painter.vline(sep, rect.y_range(), stroke);
        let handle = Rect::from_center_size(pos2(sep, rect.center().y), vec2(8.0, rect.height())).intersect(rect);
        if *col != Col::Marker && handle.width() > 0.0 {
            let resp = ui.interact(handle, ui.scope_id().with(("grid-col-resize", col.key())), Sense::click_and_drag());
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if resp.double_clicked() {
                resized = Some((i, col.default_width()));
            } else if resp.dragged() {
                resized = Some((i, (w + resp.drag_delta().x).max(col.min_width())));
            }
        }
        x = sep + spacing / 2.0;
    }
    painter.hline(rect.x_range(), rect.bottom() - 0.5, stroke);
    resized
}

struct BodyCtx<'a> {
    p: &'a Project,
    rows: &'a [RowRef],
    sel_idx: Option<usize>,
    cur_lang: Option<&'a str>,
    cols: &'a [Col],
    widths: &'a [f32],
    pal: Palette,
    issue_cells: &'a std::collections::HashSet<(RowRef, String)>,
}

fn body(ui: &mut Ui, c: &BodyCtx<'_>, scroll_to: Option<usize>, cell_edit: &mut Option<crate::app::CellEdit>, ev: &mut Events) {
    let origin = ui.cursor().min;
    let mut tb = TableBuilder::new(ui)
        .id_salt("grid")
        .striped(true)
        .resizable(false)
        .vscroll(false)
        .sense(Sense::click())
        .cell_layout(Layout::left_to_right(Align::Center));
    for &w in c.widths {
        tb = tb.column(Column::exact(w).clip(true));
    }
    // Fills the rest of the view when all columns fit.
    tb = tb.column(Column::remainder());
    tb.body(|body| {
        body.rows(ROW_HEIGHT, c.rows.len(), |mut row| {
            let i = row.index();
            let r = &c.rows[i];
            let Some(data) = c.p.row(r) else {
                return;
            };
            row.set_selected(c.sel_idx == Some(i));
            row_cells(&mut row, c, r, data, cell_edit, ev);
        });
    });
    if let Some(i) = scroll_to {
        let spacing = ui.spacing().item_spacing;
        let y = origin.y + i as f32 * (ROW_HEIGHT + spacing.y);
        // Bring the current language's column into view as well (e.g. after a click in the validation list).
        let x = c.cols.iter().position(|col| matches!(col, Col::Lang(l) if Some(l.as_str()) == c.cur_lang)).map_or_else(
            || ui.clip_rect().x_range(),
            |idx| {
                let left = origin.x + c.widths[..idx].iter().map(|w| w + spacing.x).sum::<f32>();
                egui::Rangef::new(left, left + c.widths[idx])
            },
        );
        ui.scroll_to_rect(Rect::from_x_y_ranges(x, y..=y + ROW_HEIGHT), None);
    }
}

fn click_events(resp: &egui::Response, r: &RowRef, lang: Option<&str>, ev: &mut Events) {
    if resp.clicked() || resp.secondary_clicked() {
        ev.click = Some((r.clone(), lang.map(str::to_string)));
    }
    if resp.double_clicked()
        && let Some(l) = lang
    {
        ev.double = Some((r.clone(), l.to_string()));
    }
    resp.context_menu(|ui| row_menu_items(ui, &mut ev.actions));
}

fn row_cells(row: &mut egui_extras::TableRow<'_, '_>, c: &BodyCtx<'_>, r: &RowRef, data: &Row, cell_edit: &mut Option<crate::app::CellEdit>, ev: &mut Events) {
    let original_lang = c.p.original_lang(r);
    for col in c.cols {
        if let Col::Lang(l) = col {
            let editing = cell_edit.as_ref().is_some_and(|e| e.row == *r && e.lang == *l);
            let (_, resp) = row.col(|ui| {
                if editing {
                    edit_cell(ui, cell_edit, ev);
                } else {
                    let problem = c.issue_cells.contains(&(r.clone(), l.clone()));
                    lang_cell(ui, &LangCell { row: data, lang: l, original_lang, problem }, &c.pal);
                }
            });
            if !editing {
                click_events(&resp, r, Some(l), ev);
            }
            continue;
        }
        let (_, resp) = row.col(|ui| plain_cell(ui, col, c, r, data));
        let resp = match col {
            Col::Marker => resp.on_hover_text(marker_hint(data)),
            Col::Original if has_newline(data.original()) => resp.on_hover_text(data.original()),
            _ => resp,
        };
        click_events(&resp, r, c.cur_lang, ev);
    }
    let (_, resp) = row.col(|_| {});
    click_events(&resp, r, c.cur_lang, ev);
}

/// Cells of all columns except the language columns.
fn plain_cell(ui: &mut Ui, col: &Col, c: &BodyCtx<'_>, r: &RowRef, data: &Row) {
    match col {
        Col::Marker => marker_cell(ui, data),
        Col::Context => {
            fill_cell(ui, c.pal.lavender);
            cell_label(ui, &c.p.context(r));
        }
        Col::Status => status_cell(ui, data, c.cur_lang),
        Col::Original => {
            fill_cell(ui, c.pal.lavender);
            cell_label(ui, data.original());
        }
        Col::Note => {
            cell_label(ui, data.note());
            if !data.note().is_empty() {
                comment_marker(ui, c.pal.comment);
            }
        }
        Col::TComment => {
            if let Some(t) = c.cur_lang.and_then(|l| data.translation(l)) {
                cell_label(ui, t.comment());
            }
        }
        Col::Lang(_) => {}
    }
}

fn edit_cell(ui: &mut Ui, cell_edit: &mut Option<crate::app::CellEdit>, ev: &mut Events) {
    let Some(e) = cell_edit.as_mut() else {
        return;
    };
    let resp = ui.add(egui::TextEdit::singleline(&mut e.text).desired_width(f32::INFINITY));
    if e.focus {
        resp.request_focus();
        e.focus = false;
    }
    if resp.lost_focus() {
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            ev.cancel = true;
        } else {
            ev.commit = true;
            ev.next = ui.input(|i| i.key_pressed(Key::Enter));
        }
    }
}

fn apply(app: &mut SusiApp, ev: Events) {
    if ev.cancel {
        app.cell_edit = None;
    }
    if ev.commit {
        app.commit_cell_edit();
        if ev.next {
            app.actions.push(Action::Nav(Nav::Next));
        }
    }
    if let Some((r, l)) = ev.click {
        app.select(r);
        app.scroll_to_selected = false;
        if l.is_some() {
            app.lang = l;
        }
    }
    if let Some((r, l)) = ev.double {
        let multiline = app.project.as_ref().and_then(|p| p.row(&r)).is_some_and(|row| has_newline(row.original()) || row.text(&l).is_some_and(has_newline));
        if multiline {
            app.select(r);
            app.lang = Some(l);
            app.show_bottom = true;
            app.tab = crate::app::BottomTab::Editor;
        } else {
            app.start_cell_edit(r, l);
        }
    }
    app.actions.extend(ev.actions);
}
