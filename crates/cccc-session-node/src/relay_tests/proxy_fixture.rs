//! Test-owned DNS, TLS offload and CONNECT forwarding; no public network is used.
use futures_util::future::BoxFuture;
use iroh::dns::{BoxIter, DnsError, DnsResolver, Resolver, TxtRecordData};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
};

pub const HOST: &str = "relay.proxy.invalid";
const PEM: &[u8] = include_bytes!("proxy-test-only.pem");

#[derive(Debug, Clone)]
struct LocalDns;
impl Resolver for LocalDns {
    fn lookup_ipv4(&self, host: String) -> BoxFuture<'static, Result<BoxIter<Ipv4Addr>, DnsError>> {
        Box::pin(async move {
            assert_eq!(host, HOST, "the test never queries external DNS");
            Ok(Box::new(std::iter::once(Ipv4Addr::LOCALHOST)) as BoxIter<_>)
        })
    }
    fn lookup_ipv6(&self, _: String) -> BoxFuture<'static, Result<BoxIter<Ipv6Addr>, DnsError>> {
        Box::pin(async { Ok(Box::new(std::iter::empty()) as BoxIter<_>) })
    }
    fn lookup_txt(
        &self,
        _: String,
    ) -> BoxFuture<'static, Result<BoxIter<TxtRecordData>, DnsError>> {
        Box::pin(async { Ok(Box::new(std::iter::empty()) as BoxIter<_>) })
    }
    fn clear_cache(&self) {}
    fn reset(&self) -> Box<dyn Resolver> {
        Box::new(self.clone())
    }
}

pub fn dns() -> DnsResolver {
    DnsResolver::custom(LocalDns)
}
pub fn ca() -> iroh_relay::tls::CaTlsConfig {
    iroh_relay::tls::CaTlsConfig::custom_roots([
        CertificateDer::from_pem_slice(PEM).expect("test CA")
    ])
}

pub struct Server {
    pub address: SocketAddr,
    pub connections: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub async fn tls(upstream: SocketAddr) -> Server {
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("TLS versions")
    .with_no_client_auth()
    .with_single_cert(
        vec![CertificateDer::from_pem_slice(PEM).expect("test cert")],
        PrivateKeyDer::from_pem_slice(PEM).expect("public test key"),
    )
    .expect("test TLS config");
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("TLS offload listener");
    let address = listener.local_addr().expect("TLS address");
    let connections = Arc::new(AtomicUsize::new(0));
    let accepted_count = connections.clone();
    let task = tokio::spawn(async move {
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { break; };
                    accepted_count.fetch_add(1, Ordering::SeqCst);
                    let acceptor = acceptor.clone();
                    tasks.spawn(async move {
                        if let Ok(mut tls) = acceptor.accept(stream).await
                            && let Ok(mut upstream) = TcpStream::connect(upstream).await {
                            let _ = tokio::io::copy_bidirectional(&mut tls, &mut upstream).await;
                        }
                    });
                }
            }
        }
    });
    Server {
        address,
        connections,
        task,
    }
}

pub async fn proxy(upstream: SocketAddr) -> (Server, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("CONNECT listener");
    let address = listener.local_addr().expect("proxy address");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let connections = Arc::new(AtomicUsize::new(0));
    let accepted_count = connections.clone();
    let task = tokio::spawn(async move {
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
                accepted = listener.accept() => {
                    let Ok((mut stream, _)) = accepted else { break; };
                    accepted_count.fetch_add(1, Ordering::SeqCst);
                    let captured = captured.clone();
                    tasks.spawn(async move {
                        let mut head = Vec::new();
                        while !head.ends_with(b"\r\n\r\n") && head.len() < 8192 {
                            let Ok(byte) = stream.read_u8().await else { return; };
                            head.push(byte);
                        }
                        let head = String::from_utf8(head).expect("HTTP header");
                        captured.lock().expect("capture").push(head.clone());
                        if !head.starts_with("CONNECT ") { return; }
                        let mut target = TcpStream::connect(upstream).await.expect("owned TLS target");
                        stream.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await.expect("CONNECT response");
                        let _ = tokio::io::copy_bidirectional(&mut stream, &mut target).await;
                    });
                }
            }
        }
    });
    (
        Server {
            address,
            connections,
            task,
        },
        requests,
    )
}
