<h1 align="center">async-imap</h1>
<div align="center">
 <strong>
   Async implementation of IMAP
 </strong>
</div>

<br />

<div align="center">
  <!-- Crates version -->
  <a href="https://crates.io/crates/async-imap">
    <img src="https://img.shields.io/crates/v/async-imap.svg?style=flat-square"
    alt="Crates.io version" />
  </a>
  <!-- Downloads -->
  <a href="https://crates.io/crates/async-imap">
    <img src="https://img.shields.io/crates/d/async-imap.svg?style=flat-square"
      alt="Download" />
  </a>
  <!-- docs.rs docs -->
  <a href="https://docs.rs/async-imap">
    <img src="https://img.shields.io/badge/docs-latest-blue.svg?style=flat-square"
      alt="docs.rs docs" />
  </a>
  <!-- CI -->
  <a href="https://github.com/async-email/async-imap/actions">
    <img src="https://github.com/async-email/async-imap/workflows/CI/badge.svg"
      alt="CI status" />
  </a>
</div>

<div align="center">
  <h3>
    <a href="https://docs.rs/async-imap">
      API Docs
    </a>
    <span> | </span>
    <a href="https://github.com/async-email/async-imap/releases">
      Releases
    </a>
  </h3>
</div>

<br/>

> Based on the great [rust-imap](https://crates.io/crates/imap) library.

This crate lets you connect to and interact with servers that implement the IMAP protocol ([RFC
3501](https://tools.ietf.org/html/rfc3501) and various extensions). After authenticating with
the server, IMAP lets you list, fetch, and search for e-mails, as well as monitor mailboxes for
changes. It supports at least the latest three stable Rust releases (possibly even older ones;
check the [CI results](https://travis-ci.com/jonhoo/rust-imap)).

To connect, use the [`connect`] function. This gives you an unauthenticated [`Client`]. You can
then use [`Client::login`] or [`Client::authenticate`] to perform username/password or
challenge/response authentication respectively. This in turn gives you an authenticated
[`Session`], which lets you access the mailboxes at the server.

The documentation within this crate borrows heavily from the various RFCs, but should not be
considered a complete reference. If anything is unclear, follow the links to the RFCs embedded
in the documentation for the various types and methods and read the raw text there!

Below is a basic client example. See the `examples/` directory for more.

```rust
use async_std::prelude::*;
use async_imap::error::Result;

async fn fetch_inbox_top() -> Result<Option<String>> {
    let domain = "imap.example.com";
    let tls = native_tls::TlsConnector::builder.build().unwrap().into();

    // we pass in the domain twice to check that the server's TLS
    // certificate is valid for the domain we're connecting to.
    let client = async_imap::connect((domain, 993), domain, &tls).await?;

    // the client we have here is unauthenticated.
    // to do anything useful with the e-mails, we need to log in
    let mut imap_session = client
        .login("me@example.com", "password")
        .await
        .map_err(|e| e.0)?;

    // we want to fetch the first email in the INBOX mailbox
    imap_session.select("INBOX").await?;

    // fetch message number 1 in this mailbox, along with its RFC822 field.
    // RFC 822 dictates the format of the body of e-mails
    let messages_stream = imap_session.fetch("1", "RFC822").await?;
    let messages: Vec<_> = messages_stream.collect::<Result<_>>().await?;
    let message = if let Some(m) = messages.first() {
        m
    } else {
        return Ok(None);
    };

    // extract the message's body
    let body = message.body().expect("message did not have a body!");
    let body = std::str::from_utf8(body)
        .expect("message was not valid utf-8")
        .to_string();

    // be nice to the server and log out
    imap_session.logout().await?;

    Ok(Some(body))
}
```

## Running the test suite

To run the integration tests, you need to have [GreenMail
running](http://www.icegreen.com/greenmail/#deploy_docker_standalone). The
easiest way to do that is with Docker:

```console
$ docker pull greenmail/standalone:1.5.9
$ docker run -t -i -e GREENMAIL_OPTS='-Dgreenmail.setup.test.all -Dgreenmail.hostname=0.0.0.0 -Dgreenmail.auth.disabled -Dgreenmail.verbose' -p 3025:3025 -p 3110:3110 -p 3143:3143 -p 3465:3465 -p 3993:3993 -p 3995:3995 greenmail/standalone:1.5.9
```

## License

Licensed under either of
 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
