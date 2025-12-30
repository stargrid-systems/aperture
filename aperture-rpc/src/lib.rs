use std::borrow::Cow;

use facet::Facet;

#[rapace::service]
pub trait ApertureV1 {
    async fn version(&self) -> Version<'static>;
}

#[derive(Facet)]
pub struct Version<'a> {
    pub aperture: Cow<'a, str>,
}
