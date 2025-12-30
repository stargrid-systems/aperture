pub use aperture_rpc as rpc;

pub struct Server {
    _private: (),
}

impl Server {
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl rpc::ApertureV1 for Server {
    async fn version(&self) -> rpc::Version<'static> {
        rpc::Version {
            aperture: env!("CARGO_PKG_VERSION").into(),
        }
    }
}
