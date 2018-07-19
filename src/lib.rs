extern crate futures;
extern crate new_tokio_smtp;
extern crate mail_types as mail;
extern crate mail_common;
extern crate mail_headers as headers;
#[macro_use]
extern crate failure;

mod resolve_all;

pub mod error;
mod request;
mod send_mail;

pub use self::request::*;
pub use self::send_mail::*;

pub use new_tokio_smtp::{ConnectionConfig, ConnectionBuilder};
pub use new_tokio_smtp::command::auth;
pub mod misc {
    pub use new_tokio_smtp::ClientId;
    pub use new_tokio_smtp::Domain;
    pub use new_tokio_smtp::AddressLiteral;
}