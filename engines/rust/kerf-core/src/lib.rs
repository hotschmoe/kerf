//! Kerf engine core: parse, place, validate, draw, mesh and export construction details.

pub mod annot;
pub mod api;
pub mod build;
pub mod catalog;
pub mod diag;
pub mod drawing;
pub mod export_svg;
pub mod font;
pub mod geom;
pub mod hatch;
pub mod iso;
pub mod json;
pub mod model;
pub mod num;
pub mod paper;
pub mod poly;
pub mod render;
pub mod resolve;
pub mod schema;
pub mod section;
pub mod stroke;
pub mod style;
pub mod summary;
pub mod validate;
pub mod view;

pub const ENGINE: &str = "kerf-rust";
pub const VERSION: &str = "0.1.0";
pub const SPEC: &str = "0.1";
