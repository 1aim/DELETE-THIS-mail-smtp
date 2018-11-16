# mail-smtp &emsp;

**Allows sending `mail-core` `Mail`'s through  `new-tokio-smtp`**

---


This library binds together `new-tokio-smtp` and the `mail` crates.

It can be used to send mail given  as mail crates `Mail` instances
to a Mail Submission Agent (MSA). It could, theoretically also
be used to send to an MX, but this often needs additional functionality
for reliable usage which is not part of this crate.

For ease of use this crate re-exports some of the most commonly used
parts from `new-tokio-smtp` including `ConnectionConfig`,
`ConnectionBuilder`, all authentication commands/methods (the
`auth` module) as well as useful types (in the `misc` module).

The `send_mails` function is the simplest way to send a batch
of mails. Nevertheless it doesn't directly accept `Mail` instances,
instead it accepts `MailRequest` instances. This is needed, as
the sender/recipient(s) specified through the `Mail` headers
and those used fro smtp mail delivery are not necessary exactly
the same (e.g. for bounce back mails and some no-reply setups).

# Example

```rust ,no_run
extern crate futures;
//if you use the mail facade use the re-exports from it instead
extern crate mail_core;
extern crate mail_smtp;
#[macro_use] extern crate mail_headers;

use futures::Future;
use mail_headers::*;
use mail_headers::components::Domain;
use mail_core::{Mail, default_impl::simple_context};
use mail_smtp::{send_mails, ConnectionConfig};

fn main() {
    // this is normally done _once per application instance_
    // and then stored in e.g. a lazy_static. Also `Domain`
    // will implement `FromStr` in the future.
    let ctx = simple_context::new(
        Domain::from_unchecked("example.com".to_owned(),
        // This should be "world" unique for the given domain
        // to assure message and content ids are world unique.
        "asdkds".parse().unwrap()
    ).unwrap();

    let mut mail = Mail::plain_text("Some body").unwrap();
    mail.set_headers(headers! {
        _From: ["bla@example.com"],
        _To: ["blub@example.com"],
        Subject: "Some Mail"
    }.unwrap()).unwrap();

    // don't use unencrypted con for anything but testing and
    // simplified examples
    let con_config = ConnectionConfig::build_local_unencrypted().build();

    let fut = send_mails(con_config, vec![mail.into()], ctx);
    let results = fut.wait();
}
```


## Documentation

Documentation can be [viewed on docs.rs](https://docs.rs/mail-smtp).
(once published)

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
