//! Selection of several rows in the sheet. `Ctrl`+click adds a row or removes it, `Shift`+click
//! selects the rows from the current row (the anchor) to the clicked one, `Ctrl`+`Shift`+click
//! adds them. Row commands then apply to all selected rows; toggles take the new value from the
//! anchor.

use std::collections::HashSet;

use susi_core::model::RowRef;

/// How a click changes the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// Only the clicked row.
    Single,
    /// Adds the clicked row or removes it (`Ctrl`, `Cmd` on macOS).
    Toggle,
    /// The rows from the anchor to the clicked row (`Shift`).
    Range,
    /// Adds the rows from the anchor to the clicked row (`Ctrl`+`Shift`).
    AddRange,
}

impl Click {
    pub fn of(m: egui::Modifiers) -> Self {
        match (m.command, m.shift) {
            (false, false) => Click::Single,
            (true, false) => Click::Toggle,
            (false, true) => Click::Range,
            (true, true) => Click::AddRange,
        }
    }
}

/// Anchor and selection after a click on `row`. `rows`: the rows of the sheet in their order;
/// `selection`: the selected rows including the anchor, empty when only the anchor is selected
/// (also in the result).
pub fn after_click(rows: &[RowRef], anchor: Option<&RowRef>, selection: &HashSet<RowRef>, row: &RowRef, click: Click) -> (RowRef, HashSet<RowRef>) {
    let Some(anchor) = anchor.filter(|a| rows.contains(a) && rows.contains(row)) else {
        return (row.clone(), HashSet::new());
    };
    let mut sel = if selection.contains(anchor) { selection.clone() } else { HashSet::from([anchor.clone()]) };
    let anchor = match click {
        Click::Single => return (row.clone(), HashSet::new()),
        // The last selected row cannot be removed.
        Click::Toggle if sel.contains(row) && sel.len() > 1 => {
            sel.remove(row);
            if row == anchor { first_of(rows, &sel).unwrap_or(anchor).clone() } else { anchor.clone() }
        }
        Click::Toggle => {
            sel.insert(row.clone());
            row.clone()
        }
        Click::Range | Click::AddRange => {
            if click == Click::Range {
                sel.clear();
            }
            sel.extend(range(rows, anchor, row).iter().cloned());
            anchor.clone()
        }
    };
    if sel.len() < 2 {
        sel.clear();
    }
    (anchor, sel)
}

/// The rows from `a` to `b` (both included) in the order of `rows`.
fn range<'a>(rows: &'a [RowRef], a: &RowRef, b: &RowRef) -> &'a [RowRef] {
    match (rows.iter().position(|r| r == a), rows.iter().position(|r| r == b)) {
        (Some(i), Some(j)) => &rows[i.min(j)..=i.max(j)],
        _ => &[],
    }
}

fn first_of<'a>(rows: &'a [RowRef], sel: &HashSet<RowRef>) -> Option<&'a RowRef> {
    rows.iter().find(|r| sel.contains(*r))
}

/// The rows commands apply to, in the order of `rows`: the selection, or else the anchor.
pub fn ordered(rows: &[RowRef], anchor: Option<&RowRef>, selection: &HashSet<RowRef>) -> Vec<RowRef> {
    match anchor {
        Some(a) if selection.contains(a) => rows.iter().filter(|r| selection.contains(*r)).cloned().collect(),
        _ => anchor.into_iter().cloned().collect(),
    }
}

/// Keeps the selected rows that are still shown after the sheet changed (filter, sort, search).
/// If the anchor disappeared, the first remaining row becomes the anchor.
pub fn retain_shown(rows: &[RowRef], anchor: &mut Option<RowRef>, selection: &mut HashSet<RowRef>) {
    if selection.is_empty() {
        return;
    }
    let shown: HashSet<&RowRef> = rows.iter().collect();
    selection.retain(|r| shown.contains(r));
    if !anchor.as_ref().is_some_and(|a| selection.contains(a))
        && let Some(first) = first_of(rows, selection)
    {
        *anchor = Some(first.clone());
    }
    if selection.len() < 2 {
        selection.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: usize) -> Vec<RowRef> {
        (0..n).map(|i| RowRef { source: 0, path: vec![i] }).collect()
    }

    fn set(rows: &[RowRef], idx: &[usize]) -> HashSet<RowRef> {
        idx.iter().map(|&i| rows[i].clone()).collect()
    }

    #[test]
    fn clicks_with_modifiers() {
        let r = rows(6);
        let none = HashSet::new();
        // A plain click selects one row; without an anchor every click does.
        assert_eq!(after_click(&r, Some(&r[1]), &set(&r, &[1, 2]), &r[4], Click::Single), (r[4].clone(), none.clone()));
        assert_eq!(after_click(&r, None, &none, &r[4], Click::Toggle), (r[4].clone(), none.clone()));
        // Ctrl adds the clicked row, which becomes the anchor ...
        let (a, s) = after_click(&r, Some(&r[1]), &none, &r[4], Click::Toggle);
        assert_eq!((a.clone(), s.clone()), (r[4].clone(), set(&r, &[1, 4])));
        // ... and removes it again; a removed anchor passes on to the first selected row.
        let (a, s) = after_click(&r, Some(&a), &s, &r[4], Click::Toggle);
        assert_eq!((a, s), (r[1].clone(), none.clone()));
        let (a, s) = after_click(&r, Some(&r[3]), &set(&r, &[1, 3, 5]), &r[3], Click::Toggle);
        assert_eq!((a, s), (r[1].clone(), set(&r, &[1, 5])));
        // The last row stays selected.
        assert_eq!(after_click(&r, Some(&r[2]), &none, &r[2], Click::Toggle), (r[2].clone(), none.clone()));
        // Shift selects the range from the anchor in both directions; the anchor stays.
        assert_eq!(after_click(&r, Some(&r[3]), &set(&r, &[0, 3]), &r[1], Click::Range), (r[3].clone(), set(&r, &[1, 2, 3])));
        assert_eq!(after_click(&r, Some(&r[1]), &none, &r[4], Click::Range), (r[1].clone(), set(&r, &[1, 2, 3, 4])));
        assert_eq!(after_click(&r, Some(&r[1]), &none, &r[1], Click::Range), (r[1].clone(), none.clone()));
        // Ctrl+Shift adds the range.
        assert_eq!(after_click(&r, Some(&r[4]), &set(&r, &[0, 4]), &r[5], Click::AddRange), (r[4].clone(), set(&r, &[0, 4, 5])));
        // An anchor that is not shown does not count.
        assert_eq!(after_click(&r[2..], Some(&r[0]), &none, &r[4], Click::Range), (r[4].clone(), none));
    }

    #[test]
    fn order_and_changed_views() {
        let r = rows(6);
        assert_eq!(ordered(&r, Some(&r[4]), &set(&r, &[4, 1, 2])), vec![r[1].clone(), r[2].clone(), r[4].clone()]);
        assert_eq!(ordered(&r, Some(&r[4]), &HashSet::new()), vec![r[4].clone()]);
        // A selection without the anchor is stale: only the anchor counts.
        assert_eq!(ordered(&r, Some(&r[4]), &set(&r, &[1, 2])), vec![r[4].clone()]);
        assert!(ordered(&r, None, &HashSet::new()).is_empty());

        // Rows hidden by a filter leave the selection; the anchor passes on.
        let (mut anchor, mut sel) = (Some(r[1].clone()), set(&r, &[1, 3, 4]));
        let shown = [r[0].clone(), r[3].clone(), r[4].clone()];
        retain_shown(&shown, &mut anchor, &mut sel);
        assert_eq!((anchor, sel), (Some(r[3].clone()), set(&r, &[3, 4])));
        // One row left: a single selection again.
        let (mut anchor, mut sel) = (Some(r[1].clone()), set(&r, &[1, 3]));
        retain_shown(&shown, &mut anchor, &mut sel);
        assert_eq!((anchor, sel), (Some(r[3].clone()), HashSet::new()));
        // Without a selection nothing changes, not even a hidden anchor.
        let (mut anchor, mut sel) = (Some(r[1].clone()), HashSet::new());
        retain_shown(&shown, &mut anchor, &mut sel);
        assert_eq!(anchor, Some(r[1].clone()));
    }
}
