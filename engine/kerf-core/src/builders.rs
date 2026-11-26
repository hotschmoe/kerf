use crate::catalog::Type;
use crate::model::Built;
use crate::params::{ParamSet, Row};
use alloc::vec::Vec;

pub mod anchor_bolt;
pub mod bolt;
pub mod cmu_wall;
pub mod common;
pub mod concrete;
pub mod connector;
pub mod fill;
pub mod flashing;
pub mod hardware;
pub mod ijoist;
pub mod insulation;
pub mod joint;
pub mod lumber;
pub mod membrane;
pub mod metal_deck;
pub mod nailing;
pub mod panel;
pub mod rebar;
pub mod sleeve;
pub mod solid;
pub mod stair_treads;
pub mod steel;
pub mod steel_joist;
pub mod steel_shapes;
pub mod tie;
pub mod truss;

pub use common::{Ctx, mirror_about_center, world_box};

pub fn build(ty: Type, ctx: &mut Ctx) -> Option<Built> {
    match ty {
        Type::Lumber => lumber::build(ctx),
        Type::Panel => panel::build(ctx),
        Type::CmuWall => cmu_wall::build(ctx),
        Type::Concrete => concrete::build(ctx),
        Type::Rebar => rebar::build(ctx),
        Type::AnchorBolt => anchor_bolt::build(ctx),
        Type::Connector => connector::build(ctx),
        Type::Truss => truss::build(ctx),
        Type::Ijoist => ijoist::build(ctx),
        Type::Membrane => membrane::build(ctx),
        Type::Fill => fill::build(ctx),
        Type::Insulation => insulation::build(ctx),
        Type::Solid => solid::build(ctx),
        Type::Flashing => flashing::build(ctx),
        Type::Joint => joint::build(ctx),
        Type::Sleeve => sleeve::build(ctx),
        Type::Steel => steel::build(ctx),
        Type::Bolt => bolt::build(ctx),
        Type::Nailing => nailing::build(ctx),
        Type::SteelJoist => steel_joist::build(ctx),
        Type::MetalDeck => metal_deck::build(ctx),
        Type::StairTreads => stair_treads::build(ctx),
    }
}

pub fn params_rows(ty: Type) -> Vec<Row> {
    match ty {
        Type::Lumber => lumber::Params::rows(),
        Type::Panel => panel::Params::rows(),
        Type::CmuWall => cmu_wall::Params::rows(),
        Type::Concrete => concrete::Params::rows(),
        Type::Rebar => rebar::Params::rows(),
        Type::AnchorBolt => anchor_bolt::Params::rows(),
        Type::Connector => connector::Params::rows(),
        Type::Truss => truss::Params::rows(),
        Type::Ijoist => ijoist::Params::rows(),
        Type::Membrane => membrane::Params::rows(),
        Type::Fill => fill::Params::rows(),
        Type::Insulation => insulation::Params::rows(),
        Type::Solid => solid::Params::rows(),
        Type::Flashing => flashing::Params::rows(),
        Type::Joint => joint::Params::rows(),
        Type::Sleeve => sleeve::Params::rows(),
        Type::Steel => steel::Params::rows(),
        Type::Bolt => bolt::Params::rows(),
        Type::Nailing => nailing::Params::rows(),
        Type::SteelJoist => steel_joist::Params::rows(),
        Type::MetalDeck => metal_deck::Params::rows(),
        Type::StairTreads => stair_treads::Params::rows(),
    }
}

pub fn params_has_key(ty: Type, key: &str) -> bool {
    match ty {
        Type::Lumber => lumber::Params::has_key(key),
        Type::Panel => panel::Params::has_key(key),
        Type::CmuWall => cmu_wall::Params::has_key(key),
        Type::Concrete => concrete::Params::has_key(key),
        Type::Rebar => rebar::Params::has_key(key),
        Type::AnchorBolt => anchor_bolt::Params::has_key(key),
        Type::Connector => connector::Params::has_key(key),
        Type::Truss => truss::Params::has_key(key),
        Type::Ijoist => ijoist::Params::has_key(key),
        Type::Membrane => membrane::Params::has_key(key),
        Type::Fill => fill::Params::has_key(key),
        Type::Insulation => insulation::Params::has_key(key),
        Type::Solid => solid::Params::has_key(key),
        Type::Flashing => flashing::Params::has_key(key),
        Type::Joint => joint::Params::has_key(key),
        Type::Sleeve => sleeve::Params::has_key(key),
        Type::Steel => steel::Params::has_key(key),
        Type::Bolt => bolt::Params::has_key(key),
        Type::Nailing => nailing::Params::has_key(key),
        Type::SteelJoist => steel_joist::Params::has_key(key),
        Type::MetalDeck => metal_deck::Params::has_key(key),
        Type::StairTreads => stair_treads::Params::has_key(key),
    }
}
