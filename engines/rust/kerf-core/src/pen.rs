//! The pens the drawing code draws with and the layers they land on.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pen {
    Cut,
    Profile,
    Beyond,
    Hidden,
    Hatch,
    Rebar,
    Steel,
    Membrane,
    Vapor,
    Anno,
    Dim,
    Break,
    Title,
    Frame,
}

impl Pen {
    pub const ALL: [Pen; 14] = [
        Pen::Cut, Pen::Profile, Pen::Beyond, Pen::Hidden, Pen::Hatch, Pen::Rebar, Pen::Steel,
        Pen::Membrane, Pen::Vapor, Pen::Anno, Pen::Dim, Pen::Break, Pen::Title, Pen::Frame,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Pen::Cut => "cut",
            Pen::Profile => "profile",
            Pen::Beyond => "beyond",
            Pen::Hidden => "hidden",
            Pen::Hatch => "hatch",
            Pen::Rebar => "rebar",
            Pen::Steel => "steel",
            Pen::Membrane => "membrane",
            Pen::Vapor => "vapor",
            Pen::Anno => "anno",
            Pen::Dim => "dim",
            Pen::Break => "break",
            Pen::Title => "title",
            Pen::Frame => "frame",
        }
    }

    pub fn from_name(s: &str) -> Option<Pen> {
        Pen::ALL.into_iter().find(|p| p.name() == s)
    }

    pub fn layer(self) -> LayerKey {
        match self {
            Pen::Cut | Pen::Profile | Pen::Membrane | Pen::Vapor => LayerKey::Cut,
            Pen::Beyond => LayerKey::Beyond,
            Pen::Hidden => LayerKey::Hidden,
            Pen::Hatch => LayerKey::Hatch,
            Pen::Rebar => LayerKey::Rebar,
            Pen::Steel => LayerKey::Steel,
            Pen::Anno => LayerKey::Notes,
            Pen::Dim => LayerKey::Dims,
            Pen::Break => LayerKey::Break,
            Pen::Title | Pen::Frame => LayerKey::Title,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerKey {
    Cut,
    Beyond,
    Hidden,
    Hatch,
    Steel,
    Rebar,
    Notes,
    Dims,
    Title,
    Break,
}

impl LayerKey {
    pub fn key(self) -> &'static str {
        match self {
            LayerKey::Cut => "cut",
            LayerKey::Beyond => "beyond",
            LayerKey::Hidden => "hidden",
            LayerKey::Hatch => "hatch",
            LayerKey::Steel => "steel",
            LayerKey::Rebar => "rebar",
            LayerKey::Notes => "notes",
            LayerKey::Dims => "dims",
            LayerKey::Title => "title",
            LayerKey::Break => "break",
        }
    }
}

pub const DRAW_ORDER: [LayerKey; 10] = [
    LayerKey::Hatch, LayerKey::Beyond, LayerKey::Cut, LayerKey::Steel, LayerKey::Rebar,
    LayerKey::Hidden, LayerKey::Break, LayerKey::Notes, LayerKey::Dims, LayerKey::Title,
];
