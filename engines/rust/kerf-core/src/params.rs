pub trait ParamSet: Sized {
    fn parse_all(p: &mut crate::model::Params) -> Self;
}
