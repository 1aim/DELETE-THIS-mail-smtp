use std::mem;

use new_tokio_smtp::send_mail::{
    self as smtp,
    MailAddress,
    EnvelopData
};

use mail_common::MailType;
use mail_common::encoder::{EncodingBuffer, EncodableInHeader};
use mail_common::error::EncodingError;
use headers::{Sender, _From, _To};
use headers::components::Mailbox;
use headers::error::BuildInValidationError;
use mail::Mail;
use mail::error::MailError;


#[derive(Clone, Debug)]
pub struct MailRequest {
    mail: Mail,
    envelop_data: Option<EnvelopData>
}

impl From<Mail> for MailRequest {
    fn from(mail: Mail) -> Self {
        MailRequest::new(mail)
    }
}



impl MailRequest {

    pub fn new(mail: Mail) -> Self {
        MailRequest { mail, envelop_data: None }
    }

    pub fn new_with_envelop(mail: Mail, envelop: EnvelopData) -> Self {
        MailRequest { mail, envelop_data: Some(envelop) }
    }

    pub fn override_envelop(&mut self, envelop: EnvelopData) -> Option<EnvelopData> {
        mem::replace(&mut self.envelop_data, Some(envelop))
    }

    pub fn into_mail_with_envelop(self) -> Result<(Mail, EnvelopData), MailError> {
        let envelop =
            if let Some(envelop) = self.envelop_data { envelop }
            else { derive_envelop_data_from_mail(&self.mail)? };

        Ok((self.mail, envelop))
    }
}

fn mailaddress_from_mailbox(mailbox: &Mailbox) -> Result<MailAddress, EncodingError> {
    let email = &mailbox.email;
    let needs_smtputf8 = email.check_if_internationalized();
    let mt = if needs_smtputf8 { MailType::Internationalized } else { MailType::Ascii };
    let mut buffer = EncodingBuffer::new(mt);
     {
        let mut writer = buffer.writer();
        email.encode(&mut writer)?;
        writer.commit_partial_header();
    }
    let raw: Vec<u8> = buffer.into();
    let address = String::from_utf8(raw).expect("[BUG] encoding Email produced non utf8 data");
    Ok(MailAddress::new_unchecked(address, needs_smtputf8))
}

pub fn derive_envelop_data_from_mail(mail: &Mail)
    -> Result<smtp::EnvelopData, MailError>
{
    let headers = mail.headers();
    let smtp_from =
        if let Some(sender) = headers.get_single(Sender) {
            let sender = sender?;
            //TODO double check with from field
            mailaddress_from_mailbox(sender)?
        } else {
            let from = headers.get_single(_From)
                .ok_or(BuildInValidationError::NoFrom)??;

            if from.len() > 1 {
                return Err(BuildInValidationError::MultiMailboxFromWithoutSender.into());
            }

            mailaddress_from_mailbox(from.first())?
        };

    let smtp_to =
        if let Some(to) = headers.get_single(_To) {
            let to = to?;
            to.try_mapped_ref(mailaddress_from_mailbox)?
        } else {
            return Err(BuildInValidationError::NoTo.into());
        };

    //TODO Cc, Bcc

    Ok(EnvelopData {
        from: Some(smtp_from),
        to: smtp_to
    })
}

#[cfg(test)]
mod test {

    mod derive_envelop_data_from_mail {
        use super::super::derive_envelop_data_from_mail;
        use mail::{Builder, Resource};
        use headers::{_From, _To, Sender};
        use headers::components::MediaType;
        use mail::file_buffer::FileBuffer;

        fn mock_resource() -> Resource {
            let mt = MediaType::parse("text/plain; charset=utf-8").unwrap();
            let fb = FileBuffer::new(mt, "abcd↓efg".to_owned().into());
            Resource::sourceless_from_buffer(fb)
        }

        #[test]
        fn use_sender_if_given() {
            let builder = Builder::singlepart(mock_resource())
                .header(Sender, "strange@caffe.test").unwrap()
                .header(_From, ["ape@caffe.test", "epa@caffe.test"]).unwrap()
                .header(_To, ["das@ding.test"]).unwrap();

            let mail = builder.build().unwrap();

            let envelop_data = derive_envelop_data_from_mail(&mail).unwrap();

            assert_eq!(
                envelop_data.from.as_ref().unwrap().as_str(),
                "strange@caffe.test"
            );
        }

        #[test]
        fn use_from_if_no_sender_given() {
            let builder = Builder::singlepart(mock_resource())
                .header(_From, ["ape@caffe.test"]).unwrap()
                .header(_To, ["das@ding.test"]).unwrap();

            let mail = builder.build().unwrap();

            let envelop_data = derive_envelop_data_from_mail(&mail).unwrap();

            assert_eq!(
                envelop_data.from.as_ref().unwrap().as_str(),
                "ape@caffe.test"
            );
        }

        #[test]
        fn fail_if_no_sender_but_multi_mailbox_from() {
            let builder = Builder::singlepart(mock_resource())
                .header(_From, ["ape@caffe.test", "a@b.test"]).unwrap()
                .header(_To, ["das@ding.test"]).unwrap();

            let mail = builder.build().unwrap();

            let envelop_data = derive_envelop_data_from_mail(&mail);

            //assert is_err
            envelop_data.unwrap_err();
        }

        #[test]
        fn use_to() {
            let builder = Builder::singlepart(mock_resource())
                .header(_From, ["ape@caffe.test"]).unwrap()
                .header(_To, ["das@ding.test"]).unwrap();

            let mail = builder.build().unwrap();

            let envelop_data = derive_envelop_data_from_mail(&mail).unwrap();

            assert_eq!(
                envelop_data.to.first().as_str(),
                "das@ding.test"
            );
        }
    }

    mod mailaddress_from_mailbox {
        use headers::HeaderTryFrom;
        use headers::components::{Mailbox, Email};
        use super::super::mailaddress_from_mailbox;

        #[test]
        #[cfg_attr(not(feature="test-with-traceing"), ignore)]
        fn does_not_panic_with_tracing_enabled() {
            let mb = Mailbox::try_from("hy@b").unwrap();
            mailaddress_from_mailbox(&mb).unwrap();
        }

        #[test]
        fn correctly_converts_mailbox() {
            let mb = Mailbox::from(Email::new("tast@tost.test").unwrap());
            let address = mailaddress_from_mailbox(&mb).unwrap();
            assert_eq!(address.as_str(), "tast@tost.test");
            assert_eq!(address.needs_smtputf8(), false);
        }

        #[test]
        fn tracks_if_smtputf8_is_needed() {
            let mb = Mailbox::from(Email::new("tüst@tost.test").unwrap());
            let address = mailaddress_from_mailbox(&mb).unwrap();
            assert_eq!(address.as_str(), "tüst@tost.test");
            assert_eq!(address.needs_smtputf8(), true);
        }

        #[test]
        fn puny_encodes_domain_if_smtputf8_is_not_needed() {
            let mb = Mailbox::from(Email::new("tast@tüst.test").unwrap());
            let address = mailaddress_from_mailbox(&mb).unwrap();
            assert_eq!(address.as_str(), "tast@xn--tst-hoa.test");
            assert_eq!(address.needs_smtputf8(), false);
        }

        #[test]
        fn does_not_puny_encodes_domain_if_smtputf8_is_needed() {
            let mb = Mailbox::from(Email::new("töst@tüst.test").unwrap());
            let address = mailaddress_from_mailbox(&mb).unwrap();
            assert_eq!(address.as_str(), "töst@tüst.test");
            assert_eq!(address.needs_smtputf8(), true);
        }
    }
}