# mail-smtp &emsp;

_[internal/mail-api] combines the `mail-types` crate with `new-tokio-smtp` crate_

Mainly provides a `send_mails` method, which given a `ConnectionConfig` and
a iterable source of `MailRequest`'s (e.g. `Vec<MailRequest>`) sends all mails
to the server specified in the `ConnectionConfig`. This includes setting up
the connection running a auth command, encoding all mails, sending each mail
and closing the connection afterwards.

Take a look at the [`mail-api` crate](https://github.com/1aim/mail-api) for more details.

Documentation can be [viewed on docs.rs](https://docs.rs/mail-smtp).

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
