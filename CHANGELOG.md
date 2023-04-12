# Changelog 

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Changed

- Remove `async-native-tls` dependency. TLS streams should be created by the library users as documented in `lib.rs`. #68
- Do not generate artificial "broken pipe" errors when attempting to send a request
  after reaching EOF on the response stream. #73
- Do not attempt to track if the stream is closed or not.
  `ImapStream` can wrap any kinds of streams, including streams which may become open again later,
  like files which can be rewinded after reaching end of file or appended to.

### Fixes

- Update byte-pool to 0.2.4 to fix `ensure_capacity()`.
  Previously this bug could have resulted in an attempt to read into a buffer of zero size
  and erronous detection of the end of stream.

## [0.7.0] - 2023-04-03

### Added

- Added changelog.
- Add `ID` extension support.

### Fixed

- Fix parsing of long responses by correctly setting the `decode_needs` variable. [#74](https://github.com/async-email/async-imap/pull/74).

### Changed

- Make `async-native-tls` dependency optional.
- Update to `base64` 0.21.

## [0.6.0] - 2022-06-27

### Added

- Add `QUOTA` support.
- Add `CONDSTORE` support: add `Session.select_condstore()`.
- Full tokio support.

### Fixed

- Do not ignore `SELECT` command errors.

### Changed

- Replace `rental` with `ouroboros`.
- Replace `lazy_static` with `once_cell`.

## [0.5.0] - 2021-03-23

### Changed

- Update async-std, stop-token, migrate to stable channels.

## [0.4.1] - 2020-10-14

### Fixed

- Fix response handling in authentication. [#36](https://github.com/async-email/async-imap/pull/36)

### Changed

- Update `base64` to 0.13.

## [0.3.3] - 2020-08-04

### Fixed

- [Refactor buffering, fixing infinite loop](https://github.com/async-email/async-imap/commit/9a7097dd446784257ad9a546c6f77188e983acd6). [#33](https://github.com/async-email/async-imap/pull/33)
- Updated `byte-pool` from 0.2.1 to 0.2.2 due to important bugfix.

### Changed

- [Do not try to send data when the stream is closed](https://github.com/async-email/async-imap/commit/68f21e5921a002e172d5ffadc45c62bf882a68d6).

## [0.3.2] - 2020-06-11

### Changed

- Bump `base64` to 0.12.

## [0.3.1] - 2020-05-24

### Fixed

- Ignore unsolicited responses if the channel is full.

## [0.3.0] - 2020-05-23

### Added

- Make streams and futures `Send`.

## [0.2.0] - 2020-01-04

### Added

- Added tracing logs for traffic.

### Fixed

- Correctly decode incomplete reads of long IMAP messages.
- Avoid infinite loop in decoding.
- Correct response value for manual interrupt in IDLE.
- Handle OAuth responses without challenge.
- Don't crash if we can't read the greeting from the server.
- Improved handling of unsolicited responses and errors.

### Changed

- Use thiserror for error handling.

## [0.1.1] - 2019-11-16

### Fixed

- Ensure there is enough space available when encoding.

## 0.1.0 - 2019-11-11

[0.7.0]: https://github.com/async-email/async-imap/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/async-email/async-imap/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/async-email/async-imap/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/async-email/async-imap/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/async-email/async-imap/compare/v0.3.3...v0.4.0
[0.3.3]: https://github.com/async-email/async-imap/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/async-email/async-imap/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/async-email/async-imap/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/async-email/async-imap/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/async-email/async-imap/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/async-email/async-imap/compare/v0.1.0...v0.1.1
