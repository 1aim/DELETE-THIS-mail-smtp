//! Module containing all custom errors.
use std::{io as std_io};

use new_tokio_smtp::error::{ConnectingFailed, LogicError};
use mail::error::MailError;


/// Error used when sending a mail fails.
///
/// Failing to encode a mail before sending
/// it also counts as a `MailSendError`, as
/// it's done "on the fly" when sending a mail.
#[derive(Debug, Fail)]
pub enum MailSendError {
    /// Creating the mail failed.
    ///
    /// This can happen for a number of reasons including:
    ///
    /// 1. Missing header fields.
    /// 2. Invalid header fields.
    /// 2. Encoding header fields fails.
    /// 3. Loading resources failed (resources like e.g. appendix, logo embedded in html mail, etc.)
    #[fail(display = "{}", _0)]
    Mail(MailError),

    /// Sending the mail failed.
    ///
    /// This can happen for a number of reasons including:
    /// 1. Server rejects mail transaction because of send or receiver
    ///    address or body data (e.g. body to long).
    /// 2. Mail address requires smtputf8 support, which is not given.
    /// 3. Server rejects sending the mail for other reasons (it's
    ///    closing, overloaded etc.).
    #[fail(display = "{}", _0)]
    Smtp(LogicError)
}

impl From<MailError> for MailSendError {
    fn from(err: MailError) -> Self {
        MailSendError::Mail(err)
    }
}

impl From<LogicError> for MailSendError {
    fn from(err: LogicError) -> Self {
        MailSendError::Smtp(err)
    }
}

/// Error returned when something on the transport layer failed.
///
/// Thinks causing this error include:
///
/// - TLS required but not supported by server
/// - authentication not valid
/// - connection "broke" (e.g. because you
///   internet connection is gone or the server
///   crashed)
///
#[derive(Debug, Fail)]
pub enum TransportError {
    /// Setting up the connection failed.
    ///
    /// Failures can include but are not limited to:
    ///
    /// - Connecting with TCP failed.
    /// - Starting TLS failed.
    /// - Server does not want to be used (e.g. failure on sending EHLO).
    /// - Authentication failed.
    #[fail(display = "{}", _0)]
    Connecting(ConnectingFailed),

    /// An I/O error happened while using the connection.
    ///
    /// This is mainly for I/O errors after the setup of the connection
    /// was successful, which normally includes sending Ehlo and Auth
    /// commands.
    #[fail(display = "{}", _0)]
    Io(std_io::Error)
}

impl From<std_io::Error> for TransportError {
    fn from(err: std_io::Error) -> Self {
        TransportError::Io(err)
    }
}

impl From<ConnectingFailed> for TransportError {
    fn from(err: ConnectingFailed) -> Self {
        TransportError::Connecting(err)
    }
}
