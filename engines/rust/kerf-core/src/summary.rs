//! Summary text the LLM reads after every change (SPEC section 13).

use crate::diag::{Diag, Level, count};
use crate::model::Model;
use crate::num::fmt_ftin;

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

fn pad(s: &str, n: usize) -> String {
    let len = s.chars().count();
    if len >= n { format!("{} ", s) } else { format!("{}{}", s, " ".repeat(n - len)) }
}

pub fn summary(model: &Model, nviews: usize, diags: &[Diag]) -> String {
    let (e, w, _i) = count(diags);
    let mut out = format!(
        "DOC {}  {}  {}  {}  {}\n",
        model.id,
        plural(model.comps.len(), "component", "components"),
        plural(nviews, "view", "views"),
        plural(e, "error", "errors"),
        plural(w, "warning", "warnings")
    );
    for c in &model.comps {
        let b = c.bbox();
        let mut desc = c.desc.clone();
        if let Some((axis, n, sp)) = &c.array {
            desc.push_str(&format!(" [x{} {} @ {}]", n, axis, fmt_ftin(*sp)));
        }
        if let Some(l) = &c.label {
            desc.push_str(&format!(" \"{}\"", l));
        }
        if c.failed {
            out.push_str(&format!(" {}{}(not built: see errors)\n", pad(&c.id, 14), pad(&c.ctype, 44)));
            continue;
        }
        let xr = format!("x {}..{}", fmt_ftin(b.x0), fmt_ftin(b.x1));
        let yr = format!("y {}..{}", fmt_ftin(b.y0), fmt_ftin(b.y1));
        out.push_str(&format!(" {}{}{}{}\n", pad(&c.id, 14), pad(&desc, 44), pad(&xr, 18), yr));
    }
    for d in diags {
        if d.level == Level::Info && d.code != "I_SOLID_USED" && d.code != "I_UNVERIFIED_CITE" && d.code != "I_CITE_DOWNGRADED" {
            continue;
        }
        out.push_str(&d.line());
        out.push('\n');
    }
    out
}
