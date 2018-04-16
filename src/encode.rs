use std::io::{Error as IoError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::mem;

use futures::sync::{mpsc, oneshot};
use futures::{future, Future, Stream, Poll, Async};
use futures::stream::BufferUnordered;

use new_tokio_smtp::Connection;

use mail::prelude::{Encoder, Encodable, MailType};
use mail::utils::SendBoxFuture;
use mail::context::BuilderContext;

use super::smtp_wrapper::{send_mail, close_smtp_conn};
use super::common::{
    MailSendResult, MailResponse, MailRequest, EnvelopData,
    Handle2ServiceMsg
};
use super::handle::MailServiceHandle;
use super::error::MailSendError;

pub(crate) type MailEncodingResult = Result<(Vec<u8>, EnvelopData), MailSendError>;

/// encodes the mails in the input stream in a thread poll returning the results out of order
///
/// The `max_concurrent` parameter determines how many mails are encoded concurrently.
///
/// Note that it uses `ctx.offload` to offload work, i.e. send it to a thread pool,
/// if `ctx.offload` is not implemented in terms of a thread pool this will not encode
/// mail in a thread poll but with whatever mechanism `ctx.offload` uses to resolve
/// futures.
///
pub(crate) fn stream_encode_mail<S, CTX>(steam: S, ctx: CTX, max_concurrent: usize)
    //FIXME[rust/impl Trait]: use impl Trait instead of boxing
    -> Box<Stream<
        //FIXME[futures >= 0.2]: replace () with Never
        Item=SendBoxFuture<(MailEncodingResult, oneshot::Sender<MailSendResult>), ()>,
        //FIXME[futures >= 0.2]: replace () with Never
        Error=()>>
    //FIXME[futures >= 0.2]: replace () with Never
    where S: Stream<Item=Handle2ServiceMsg, Error=()>, CTX: BuilderContext
{
    let _ctx = ctx;
    let fut_stream = stream.map(move |(req, tx)| {
        //clone ctx to move it into the operation chain
        let ctx = _ctx.clone();
        let operation = future
                    //use lazy to make sure it's run in the thread pool
                    ::lazy(move || mail_request.into_mail_with_envelop())
                    .then(move |result| match result {
                        Ok((mail, envelop)) => Ok((mail, envelop, tx)),
                        Err(err) => Err((MailSendError::CreatingEnvelop(err), tx))
                    })
                    .and_then(move |(mail, envelop, tx)| {
                        mail.into_encodeable_mail(&ctx)
                            .then(move |result| match result {
                                Ok(enc_mail) => Ok((enc_mail, envelop, tx)),
                                Err(err) => Err((MailSendError::Encoding(err), tx))
                            })
                    })
                    .and_then(move |(encodable_mail, envelop, tx)| {
                        //TODO we need to feed in the MailType (and get it from tokio smtp)
                        let mut encoder = Encoder::new( MailType::Ascii );
                        match encodable_mail.encode(&mut encoder) {
                            Ok(()) => {},
                            Err(err) => return Err((MailSendError::Encoding(err), tx))
                        }

                        let bytes = match encoder.to_vec() {
                            Ok(bytes) => bytes,
                            Err(err) => return Err((MailSendError::Encoding(err), tx))
                        };

                        //TODO we also need to return SmtpEnvelop<Vec<u8>>
                        let enc_result = Ok((bytes, envelop));
                        Ok((enc_result, tx))
                    })
                    .or_else(move |(err, tx)| {
                        let enc_result = Err(err);
                        Ok((enc_result, tx))
                    });

            // offload work to thread pool
            let fut = _ctx.offload(operation);


            // return future as new item
            fut
        });

    // buffer
    let buffered = fut_stream.buffer_unordered(max_concurrent);

    Box::new(buffered)
}

