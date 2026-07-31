use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) async fn prompt_page() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind page");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await;
            let body = "<textarea id='prompt-textarea' autofocus></textarea><script>prompt=document.querySelector('textarea');prompt.addEventListener('keydown',event=>{if(event.key==='Enter'){event.preventDefault();globalThis.submitted=prompt.value;}})</script>";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });
    (format!("http://{address}"), server)
}

pub(super) fn chrome_available() -> bool {
    [
        "/opt/homebrew/bin/chromium",
        "/usr/bin/chromium",
        "/usr/bin/google-chrome",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    ]
    .iter()
    .any(|path| std::path::Path::new(path).is_file())
}
