use crate::Custom;

pub mod hello_world {
    tonic::include_proto!("helloworld");
}

mod dashboard;
mod jammer;
mod snapshot;

#[derive(Clone)]
pub struct MyJammer {
    data_cache: Custom,
}

impl MyJammer {
    pub fn new(data_cache: Custom) -> Self {
        Self { data_cache }
    }
}
