//! Iso view with hidden-line removal (SPEC section 8.2). Stub for the vertical slice.

use crate::diag::Diag;
use crate::geom::*;
use crate::model::Model;
use crate::section::{ViewBase, VisInfo};
use crate::style::Style;
use crate::view::ViewParams;

pub fn build_iso(_model: &Model, vp: &ViewParams, _style: &Style, _diags: &mut Vec<Diag>) -> ViewBase {
    let crop = vp.crop.unwrap_or(Rect::new(0.0, 0.0, 12.0, 12.0));
    ViewBase { items: vec![], vis: vec![], crop, s: 12.0 }
}

pub fn iso_landing(_target: &str, _vis: &[VisInfo], _model: &Model, _crop: &Rect) -> Option<Pt> {
    None
}

pub fn project_point(_vp: &ViewParams, _model: &Model, _p: Pt) -> Option<Pt> {
    None
}
