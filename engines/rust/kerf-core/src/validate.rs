//! Validation diagnostics (SPEC section 9).

use crate::diag::Diag;
use crate::model::Model;
use crate::style::Style;
use serde_json::Value;

pub fn validate(_doc: &Value, _model: &Model, _style: &Style, _diags: &mut Vec<Diag>) {}
