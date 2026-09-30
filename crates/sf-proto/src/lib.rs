//! gRPC-Client-Stubs für die OneHub-API der STARFACE (Port 9092).
//!
//! Die Protos unter `proto/` sind aus der STARFACE App für Windows
//! rekonstruiert (Version in `proto/VERSION`). Die Module folgen den
//! Protobuf-Paketen, z. B. [`gamma::onehub::api::v1::me`].

#![allow(clippy::all, clippy::pedantic)]

include!(concat!(env!("OUT_DIR"), "/onehub.rs"));

pub use gamma::onehub::api::v1;
