use std::io::{Error as IoError};
use ::error::Error;
use new_tokio_smtp::error::LogicError;

#[derive(Debug)]
pub enum MailSendError {
    CreatingEnvelop(EnvelopFromMailError),
    Composition(Error),
    Encoding(Error),
    //Note with pipelining this will change to Vec<LogicError>
    Smtp(LogicError),
    Io(IoError),
    DriverDropped,
    CanceledByDriver
}


#[derive(Debug)]
pub enum EnvelopFromMailError {
    NeitherSenderNorFrom,
    TypeError(Error),
    NoSenderAndMoreThanOneFrom,
    NoToHeaderField
}
