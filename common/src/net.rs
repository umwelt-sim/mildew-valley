//! What every mildew-valley binary shares for networking.
//!
//! Argument parsing, NATS connection, and the QUIC endpoint the edge listens
//! on. Deliberately outside umwelt: these are deployment decisions a consumer
//! makes, not things the library provides.

use std::str::FromStr;

pub const DEFAULT_NATS: &str = "nats://127.0.0.1:4222";
pub const DEFAULT_EDGE: &str = "127.0.0.1:7777";
pub const ALPN: &[u8] = b"umwelt-mildew";

/// `--name value`, anywhere in the arguments.
pub fn arg(name: &str) -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == format!("--{name}") {
            return args.next();
        }
    }
    None
}

pub fn arg_or<T: FromStr>(name: &str, fallback: T) -> T {
    match arg(name) {
        Some(raw) => raw.parse().unwrap_or_else(|_| {
            eprintln!("--{name}: cannot read {raw:?}");
            std::process::exit(2);
        }),
        None => fallback,
    }
}

/// Connects to NATS, with credentials if a path was given.
pub async fn connect(
    url: &str,
    creds: Option<String>,
) -> Result<async_nats::Client, Box<dyn std::error::Error + Send + Sync>> {
    let options = match creds {
        Some(path) => async_nats::ConnectOptions::with_credentials_file(path).await?,
        None => async_nats::ConnectOptions::new(),
    };
    let servers: Vec<async_nats::ServerAddr> =
        url.split(',').map(|one| one.trim().parse()).collect::<Result<_, _>>()?;
    Ok(async_nats::connect_with_options(servers, options).await?)
}

/// Installs the crypto provider, once per process.
pub fn provider() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = quinn::rustls::crypto::ring::default_provider().install_default();
    });
}

/// A client endpoint that accepts whatever certificate the edge presents.
///
/// For local development only. A deployment builds its own endpoint against
/// the roots its operator chose.
pub fn game_endpoint(runtime: &tokio::runtime::Handle) -> quinn::Endpoint {
    provider();
    let mut tls = quinn::rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(TrustAnything))
        .with_no_client_auth();
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let tls =
        quinn::crypto::rustls::QuicClientConfig::try_from(tls).expect("a TLS 1.3 config");

    let _guard = runtime.enter();
    let mut endpoint =
        quinn::Endpoint::client("0.0.0.0:0".parse().expect("a valid address"))
            .unwrap_or_else(|e| {
                eprintln!("binding a client socket: {e}");
                std::process::exit(1);
            });
    endpoint.set_default_client_config(quinn::ClientConfig::new(std::sync::Arc::new(tls)));
    endpoint
}

#[derive(Debug)]
struct TrustAnything;

impl quinn::rustls::client::danger::ServerCertVerifier for TrustAnything {
    fn verify_server_cert(
        &self,
        _: &quinn::rustls::pki_types::CertificateDer<'_>,
        _: &[quinn::rustls::pki_types::CertificateDer<'_>],
        _: &quinn::rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: quinn::rustls::pki_types::UnixTime,
    ) -> Result<quinn::rustls::client::danger::ServerCertVerified, quinn::rustls::Error>
    {
        Ok(quinn::rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &quinn::rustls::pki_types::CertificateDer<'_>,
        _: &quinn::rustls::DigitallySignedStruct,
    ) -> Result<
        quinn::rustls::client::danger::HandshakeSignatureValid,
        quinn::rustls::Error,
    > {
        Ok(quinn::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &quinn::rustls::pki_types::CertificateDer<'_>,
        _: &quinn::rustls::DigitallySignedStruct,
    ) -> Result<
        quinn::rustls::client::danger::HandshakeSignatureValid,
        quinn::rustls::Error,
    > {
        Ok(quinn::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<quinn::rustls::SignatureScheme> {
        quinn::rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// A listening QUIC endpoint with a self-signed certificate.
///
/// Fine for local development. A deployment builds its own endpoint from
/// whatever its operator actually trusts.
pub fn edge_endpoint(addr: &str, runtime: &tokio::runtime::Handle) -> quinn::Endpoint {
    provider();
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])
        .unwrap_or_else(|e| {
            eprintln!("generating a certificate: {e}");
            std::process::exit(1);
        });
    let key = quinn::rustls::pki_types::PrivateKeyDer::try_from(
        cert.signing_key.serialize_der(),
    )
    .expect("a key rcgen just produced");

    let mut tls = quinn::rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert.cert.der().clone()], key)
        .unwrap_or_else(|e| {
            eprintln!("server tls: {e}");
            std::process::exit(1);
        });
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let tls =
        quinn::crypto::rustls::QuicServerConfig::try_from(tls).expect("a TLS 1.3 config");
    let config = quinn::ServerConfig::with_crypto(std::sync::Arc::new(tls));

    let addr: std::net::SocketAddr = addr.parse().unwrap_or_else(|e| {
        eprintln!("--edge {addr:?}: {e}");
        std::process::exit(1);
    });
    let _guard = runtime.enter();
    quinn::Endpoint::server(config, addr).unwrap_or_else(|e| {
        eprintln!("binding {addr}: {e}");
        std::process::exit(1);
    })
}
