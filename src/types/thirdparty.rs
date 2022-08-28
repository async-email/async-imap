//! Contains types that are from third-party dependencies that are used in the public API

use chrono::{DateTime, FixedOffset};

/// TlsStream re-exports [`async_native_tls::TlsStream`].
pub type TlsStream<T> = async_native_tls::TlsStream<T>;

#[cfg(feature = "runtime-async-std")]
/// TcpStream re-exports [`async_std::net::TcpStream`].
pub type TcpStream = async_std::net::TcpStream;

#[cfg(feature = "runtime-tokio")]
/// TcpStream re-exports [`tokio::net::TcpStream`].
pub type TcpStream = tokio::net::TcpStream;

/// TlsConnector re-exports [`async_native_tls::TlsConnector`].
pub type TlsConnector = async_native_tls::TlsConnector;

/// FixedOffsetDateTime is an alias for a fixed-offset ([`FixedOffset`]) [`DateTime`]
pub type FixedOffsetDateTime = DateTime<FixedOffset>;

/// StopSource re-exports [`stop_token::StopSource`]
pub type StopSource = stop_token::StopSource;
