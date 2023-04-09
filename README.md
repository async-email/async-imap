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

See the `examples/` directory for examples.

## Running the test suite

To run the integration tests, you need to have [GreenMail
running](https://greenmail-mail-test.github.io/greenmail/#deploy_docker_standalone). The
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
