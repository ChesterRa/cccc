use std::future::Future;

pub(super) async fn bind_web_listener(
    host: &str,
    port: u16,
) -> std::io::Result<tokio::net::TcpListener> {
    bind_web_listener_with(host, port, cfg!(windows), |host, port| async move {
        tokio::net::TcpListener::bind((host, port)).await
    })
    .await
}

pub(super) async fn bind_web_listener_with<B, Fut>(
    host: &str,
    port: u16,
    windows: bool,
    mut bind: B,
) -> std::io::Result<tokio::net::TcpListener>
where
    B: FnMut(String, u16) -> Fut,
    Fut: Future<Output = std::io::Result<tokio::net::TcpListener>>,
{
    match bind(host.to_owned(), port).await {
        Ok(listener) => Ok(listener),
        Err(error) if windows && error.raw_os_error() == Some(10013) && port != 0 => {
            tracing::warn!(
                requested_port = port,
                "Windows denied the requested Web port (it may be reserved by Hyper-V/WinNAT); retrying with an OS-assigned port"
            );
            bind(host.to_owned(), 0).await
        }
        Err(error) => Err(error),
    }
}
