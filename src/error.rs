use std::{io as std_io};

use new_tokio_smtp::error::{ConnectingFailed, LogicError};
use mail::error::MailError;


#[derive(Debug, Fail)]
pub enum MailSendError {
    /// creating the mail failed
    ///
    /// This can happen because of a number of reasons including:
    ///
    /// 1. missing header fields
    /// 2. invalid header fields
    /// 2. encoding header fields fails
    /// 3. loading resources failed
    ///    (resources like e.g. appendix, logo embedded in html mail, etc.)
    ///
    #[fail(display = "{}", _0)]
    Mail(MailError),

    /// sending the mail failed
    ///
    /// This can happen because of a number of reasons including:
    /// 1. server rejects mail transaction because of send or receiver
    ///    address or body data (e.g. body to long).
    /// 2. mail address requires smtputf8 support, which is not given
    /// 3. server rejects sending the mail for other reasons (it's
    ///    closing, overloaded etc.)
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

#[derive(Debug, Fail)]
pub enum TransportError {

    /// Setting up the connection failed
    ///
    /// Failures can include but are not limited to:
    ///
    /// - connecting with tcp failed
    /// - starting tls failed
    /// - server does not want to be used (e.g. failure on sending EHLO)
    /// - authentication failed
    #[fail(display = "{}", _0)]
    Connecting(ConnectingFailed),

    /// An I/O-Error happened while using the connection
    ///
    /// This is mainly for I/O-Error after the setup of the connection
    /// was successful, which normally sending includes Ehlo and Auth
    /// commands
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
