extern crate new_tokio_smtp;

mod stop_handle;
mod mpsc_ext;

pub mod error;
mod common;
mod smtp_wrapper;
mod encode;
mod handle;
mod service;

pub use self::stop_handle::StopServiceHandle;
pub use self::common::*;
pub use self::handle::*;
pub use self::service::*;

#[cfg(test)]
mod test;