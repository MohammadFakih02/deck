//! HTTPS for the phone remote, on the same port as plain HTTP.
//!
//! Phone browsers only let a page use the microphone (and AudioWorklet, WebCodecs) on a secure
//! page. Deck has no name a public certificate could be made for, so it makes its own: a
//! self-signed certificate for this PC's addresses, kept in `phone-tls.json` (the key sealed for
//! this Windows account). The phone warns once ("not private") and remembers the choice.
//!
//! One port serves both: the first byte of a connection tells them apart (22 starts every TLS
//! handshake; an HTTP request starts with a letter). Handshakes run on their own tasks, so a
//! slow or silent connection never holds up the others.

use crate::util::write_atomic;
use axum::serve::Listener;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::server::TlsStream;
use tokio_util::either::Either;

pub type Io = Either<TcpStream, TlsStream<TcpStream>>;

#[derive(Serialize, Deserialize, Default)]
struct Stored {
    /// the addresses and names the certificate is for
    names: Vec<String>,
    cert: String,
    /// sealed (crate::secret)
    key: String,
}

fn path() -> std::path::PathBuf {
    crate::settings::Settings::dir().join("phone-tls.json")
}

/// The names the certificate should cover now: this PC's home and Tailscale addresses, its name.
fn wanted_names() -> Vec<String> {
    let mut n = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    if let Ok(pc) = std::env::var("COMPUTERNAME") {
        if !pc.is_empty() {
            n.push(pc.to_lowercase());
        }
    }
    n.extend(crate::net::lan_ip());
    n.extend(crate::rdesk::tailscale_ip());
    n
}

fn make(names: &[String]) -> Option<Stored> {
    let mut p = rcgen::CertificateParams::new(names.to_vec()).ok()?;
    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, format!("Deck on {}", std::env::var("COMPUTERNAME").unwrap_or_else(|_| "this PC".into())));
    p.distinguished_name = dn;
    p.not_before = rcgen::date_time_ymd(2025, 1, 1);
    p.not_after = rcgen::date_time_ymd(2045, 1, 1);
    let key = rcgen::KeyPair::generate().ok()?;
    let cert = p.self_signed(&key).ok()?;
    Some(Stored { names: names.to_vec(), cert: cert.pem(), key: crate::secret::seal_str(&key.serialize_pem()) })
}

/// The stored certificate, or a new one when there is none or this PC's address isn't on it (a
/// new router, Tailscale turned on): the phone then warns once more.
fn load_or_make() -> Option<(String, String)> {
    let names = wanted_names();
    let old: Stored = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let key = crate::secret::unseal_str(&old.key);
    if !old.cert.is_empty() && !key.is_empty() && names.iter().all(|n| old.names.contains(n)) {
        return Some((old.cert, key));
    }
    // keep the addresses the old one had too, so switching between networks doesn't keep making new
    // ones; today's first, so the oldest are the ones left out
    let mut all = names;
    for n in old.names {
        if !all.contains(&n) {
            all.push(n);
        }
    }
    all.truncate(16);
    let s = make(&all)?;
    let key = crate::secret::unseal_str(&s.key);
    if let Ok(t) = serde_json::to_vec_pretty(&s) {
        if write_atomic(&path(), &t).is_err() {
            crate::log::warn("phone https: the certificate couldn't be saved; a new one is made next start");
        }
    }
    crate::log::info("phone https: made a certificate for this PC's addresses");
    Some((s.cert, key))
}

fn server_config() -> Option<Arc<tokio_rustls::rustls::ServerConfig>> {
    use tokio_rustls::rustls;
    let (cert, key) = load_or_make()?;
    let certs: Vec<_> = pem_certs(&cert)?;
    let key = rustls::pki_types::PrivateKeyDer::try_from(pem_der(&key, "PRIVATE KEY")?).ok()?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .ok()?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .ok()?;
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Some(Arc::new(cfg))
}

/// The DER bytes of the first PEM block labelled `label`.
fn pem_der(pem: &str, label: &str) -> Option<Vec<u8>> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let a = pem.find(&begin)? + begin.len();
    let b = a + pem[a..].find(&end)?;
    let b64: String = pem[a..b].chars().filter(|c| !c.is_whitespace()).collect();
    crate::util::unbase64(&b64)
}

fn pem_certs(pem: &str) -> Option<Vec<tokio_rustls::rustls::pki_types::CertificateDer<'static>>> {
    Some(vec![tokio_rustls::rustls::pki_types::CertificateDer::from(pem_der(pem, "CERTIFICATE")?)])
}

/// A listener for axum that answers HTTP and HTTPS on one port.
pub struct Mixed {
    rx: tokio::sync::mpsc::Receiver<(Io, SocketAddr)>,
    addr: SocketAddr,
}

impl Mixed {
    /// Without a certificate (it couldn't be made) the port still answers plain HTTP.
    pub fn new(listener: TcpListener) -> Self {
        let tls = server_config().map(tokio_rustls::TlsAcceptor::from);
        if tls.is_none() {
            crate::log::warn("phone https: no certificate, plain HTTP only");
        }
        Self::with(listener, tls)
    }

    fn with(listener: TcpListener, tls: Option<tokio_rustls::TlsAcceptor>) -> Self {
        let addr = listener.local_addr().unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        tokio::spawn(async move {
            loop {
                // the server was stopped (the phone remote turned off): end, which frees the port
                let accepted = tokio::select! {
                    a = listener.accept() => a,
                    _ = tx.closed() => break,
                };
                let (tcp, peer) = match accepted {
                    Ok(x) => x,
                    Err(_) => {
                        // out of sockets and the like: wait a little rather than spin
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                };
                // no Nagle: the Screen view's pictures and the phone's answers go out at once
                let _ = tcp.set_nodelay(true);
                let (tx, tls) = (tx.clone(), tls.clone());
                tokio::spawn(async move {
                    let mut first = [0u8; 1];
                    let Ok(Ok(n)) = tokio::time::timeout(Duration::from_secs(10), tcp.peek(&mut first)).await else { return };
                    let io = match (n, first[0], tls) {
                        (1, 22, Some(acc)) => match tokio::time::timeout(Duration::from_secs(10), acc.accept(tcp)).await {
                            Ok(Ok(s)) => Either::Right(s),
                            // the phone said no to the certificate (the warning page) or left
                            _ => return,
                        },
                        _ => Either::Left(tcp),
                    };
                    let _ = tx.send((io, peer)).await;
                });
            }
        });
        Self { rx, addr }
    }
}

impl Listener for Mixed {
    type Io = Io;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.rx.recv().await {
            Some(x) => x,
            // the accept task ended (it never does on its own): wait for the server to be stopped
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(self.addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_made_certificate_reads_back_for_rustls() {
        let s = make(&["localhost".into(), "192.168.1.20".into()]).expect("certificate");
        let key = crate::secret::unseal_str(&s.key);
        assert!(pem_der(&s.cert, "CERTIFICATE").is_some_and(|d| d.len() > 100));
        let der = pem_der(&key, "PRIVATE KEY").expect("key");
        assert!(tokio_rustls::rustls::pki_types::PrivateKeyDer::try_from(der).is_ok());
    }

    /// Turning the phone remote off and on again finds its own port free (it used to move on to
    /// the next one, and the old one stayed open).
    #[tokio::test]
    async fn a_stopped_server_frees_its_port() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let mut m = Mixed::with(l, None);
        let mut c = TcpStream::connect(addr).await.unwrap();
        // the first byte tells HTTP from HTTPS
        tokio::io::AsyncWriteExt::write_all(&mut c, b"G").await.unwrap();
        let (_io, _) = tokio::time::timeout(Duration::from_secs(5), m.accept()).await.expect("a plain connection comes through");
        drop(m);
        let mut ok = false;
        for _ in 0..50 {
            if TcpListener::bind(addr).await.is_ok() {
                ok = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(ok, "the port is free again");
    }
}
