use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Copy)]
pub(super) enum PromptPageBehavior {
    Submit,
    AttachmentPreviewClearsInput,
    DuplicateAttachmentDialog,
    IgnoreSend,
    StopOnly,
    AutoSubmitThenStop,
    LegacyStaged,
    LegacyEdited,
}

pub(super) async fn prompt_page() -> (String, tokio::task::JoinHandle<()>) {
    prompt_page_with(PromptPageBehavior::Submit).await
}

pub(super) async fn prompt_page_with(
    behavior: PromptPageBehavior,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind page");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await;
            let body = match behavior {
                PromptPageBehavior::Submit => {
                    r#"<!doctype html>
<html>
<body>
  <textarea aria-label="Chat with ChatGPT" placeholder="Ask ChatGPT" style="display:none"></textarea>
  <main>
    <form id="composer-form">
      <div class="prosemirror-parent">
        <div id="prompt-textarea" class="ProseMirror" role="textbox" contenteditable="true" style="width:440px;min-height:64px"></div>
      </div>
      <input id="upload-photos" type="file" accept="image/*" multiple style="display:none" />
      <button type="button" data-testid="send-button" aria-label="Send prompt">Send</button>
    </form>
    <section id="messages"></section>
  </main>
  <script>
    const prompt = document.querySelector('#prompt-textarea');
    document.querySelector('[data-testid="send-button"]').addEventListener('click', () => {
      globalThis.submitted = prompt.innerText || prompt.textContent || '';
      globalThis.submittedFiles = Array.from(document.querySelector('#upload-photos').files || []).map(file => file.name);
      globalThis.sendClicks = (globalThis.sendClicks || 0) + 1;
      const message = document.createElement('article');
      message.setAttribute('data-message-author-role', 'user');
      message.textContent = globalThis.submitted;
      document.querySelector('#messages').appendChild(message);
      prompt.textContent = '';
      prompt.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'deleteContentBackward' }));
      history.pushState({}, '', '/c/test-conversation');
    });
  </script>
</body>
</html>"#
                }
                PromptPageBehavior::AttachmentPreviewClearsInput => {
                    r#"<!doctype html>
<html>
<body>
  <main>
    <form id="composer-form">
      <div id="preview-host"></div>
      <div class="prosemirror-parent">
        <div id="prompt-textarea" class="ProseMirror" role="textbox" contenteditable="true" style="width:440px;min-height:64px"></div>
      </div>
      <input id="upload-photos" type="file" accept="image/*" multiple style="display:none" />
      <button type="button" data-testid="send-button" aria-label="Send prompt">Send</button>
    </form>
    <section id="messages"></section>
  </main>
  <script>
    const prompt = document.querySelector('#prompt-textarea');
    const bindUpload = input => input.addEventListener('change', () => {
      globalThis.uploadAttempts = (globalThis.uploadAttempts || 0) + 1;
      globalThis.lastFileName = input.files?.[0]?.name || '';
      if (!document.querySelector('#generic-preview')) {
        const preview = document.createElement('div');
        preview.id = 'generic-preview';
        preview.innerHTML = '<img alt="" src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==" style="width:48px;height:48px" />';
        document.querySelector('#preview-host').appendChild(preview);
      }
      const replacement = input.cloneNode(true);
      input.replaceWith(replacement);
      bindUpload(replacement);
    });
    bindUpload(document.querySelector('#upload-photos'));
    document.querySelector('[data-testid="send-button"]').addEventListener('click', () => {
      globalThis.submitted = prompt.innerText || prompt.textContent || '';
      globalThis.submittedFiles = globalThis.lastFileName ? [globalThis.lastFileName] : [];
      globalThis.sendClicks = (globalThis.sendClicks || 0) + 1;
      const message = document.createElement('article');
      message.setAttribute('data-message-author-role', 'user');
      message.textContent = globalThis.submitted;
      document.querySelector('#messages').appendChild(message);
      prompt.textContent = '';
    });
  </script>
</body>
</html>"#
                }
                PromptPageBehavior::DuplicateAttachmentDialog => {
                    r#"<!doctype html>
<html>
<body>
  <main>
    <form id="composer-form">
      <div><img alt="" src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==" style="width:48px;height:48px" /></div>
      <div id="prompt-textarea" class="ProseMirror" role="textbox" contenteditable="true" style="width:440px;min-height:64px"></div>
      <input id="upload-photos" type="file" accept="image/*" multiple style="display:none" />
      <button type="button" data-testid="send-button" aria-label="Send prompt">Send</button>
    </form>
    <section id="messages"></section>
  </main>
  <div role="dialog" aria-modal="true" style="position:fixed;inset:0;width:600px;height:400px;background:white">
    <h2>You've already uploaded this file.</h2>
    <p>Try uploading something new.</p>
    <button type="button">OK</button>
  </div>
  <script>
    const prompt = document.querySelector('#prompt-textarea');
    document.querySelector('#upload-photos').addEventListener('change', () => {
      globalThis.uploadAttempts = (globalThis.uploadAttempts || 0) + 1;
    });
    document.querySelector('[role="dialog"] button').addEventListener('click', event => {
      globalThis.dialogDismissals = (globalThis.dialogDismissals || 0) + 1;
      event.currentTarget.closest('[role="dialog"]').remove();
    });
    document.querySelector('[data-testid="send-button"]').addEventListener('click', () => {
      globalThis.submitted = prompt.innerText || prompt.textContent || '';
      globalThis.sendClicks = (globalThis.sendClicks || 0) + 1;
      const message = document.createElement('article');
      message.setAttribute('data-message-author-role', 'user');
      message.textContent = globalThis.submitted;
      document.querySelector('#messages').appendChild(message);
      prompt.textContent = '';
    });
  </script>
</body>
</html>"#
                }
                PromptPageBehavior::IgnoreSend => {
                    r#"<!doctype html>
<html>
<body>
  <textarea aria-label="Chat with ChatGPT" placeholder="Ask ChatGPT" style="display:none"></textarea>
  <main>
    <form id="composer-form">
      <div id="prompt-textarea" class="ProseMirror" role="textbox" contenteditable="true" style="width:440px;min-height:64px"></div>
      <button type="button" data-testid="send-button" aria-label="Send prompt">Send</button>
    </form>
  </main>
  <script>
    document.querySelector('[data-testid="send-button"]').addEventListener('click', () => {
      globalThis.sendClicks = (globalThis.sendClicks || 0) + 1;
    });
  </script>
</body>
</html>"#
                }
                PromptPageBehavior::StopOnly => {
                    r#"<!doctype html>
<html>
<body>
  <textarea aria-label="Chat with ChatGPT" placeholder="Ask ChatGPT" style="display:none"></textarea>
  <main>
    <form id="composer-form">
      <div id="prompt-textarea" class="ProseMirror" role="textbox" contenteditable="true" style="width:440px;min-height:64px"></div>
      <button type="button" data-testid="stop-button" aria-label="Stop generating">Stop</button>
    </form>
  </main>
  <script>
    document.querySelector('[data-testid="stop-button"]').addEventListener('click', () => {
      globalThis.stopClicks = (globalThis.stopClicks || 0) + 1;
    });
  </script>
</body>
</html>"#
                }
                PromptPageBehavior::AutoSubmitThenStop => {
                    r#"<!doctype html>
<html>
<body>
  <main>
    <form id="composer-form">
      <div id="prompt-textarea" class="ProseMirror" role="textbox" contenteditable="true" style="width:440px;min-height:64px"></div>
    </form>
    <section id="messages"></section>
  </main>
  <script>
    const prompt = document.querySelector('#prompt-textarea');
    const observer = new MutationObserver(() => {
      const text = prompt.innerText || prompt.textContent || '';
      if (!text || globalThis.autoSubmitScheduled) return;
      globalThis.autoSubmitScheduled = true;
      window.setTimeout(() => {
        globalThis.submitted = text;
        globalThis.sendClicks = (globalThis.sendClicks || 0) + 1;
        const message = document.createElement('article');
        message.setAttribute('data-message-author-role', 'user');
        message.textContent = text;
        document.querySelector('#messages').appendChild(message);
        prompt.textContent = '';
        const stop = document.createElement('button');
        stop.type = 'button';
        stop.dataset.testid = 'stop-button';
        stop.setAttribute('aria-label', 'Stop generating');
        stop.textContent = 'Stop';
        document.querySelector('#composer-form').appendChild(stop);
        history.pushState({}, '', '/c/auto-submitted-conversation');
      }, 50);
    });
    observer.observe(prompt, { childList: true, characterData: true, subtree: true });
  </script>
</body>
</html>"#
                }
                PromptPageBehavior::LegacyStaged => {
                    r#"<!doctype html>
<html>
<body>
  <main>
    <form id="composer-form">
      <div id="prompt-textarea" class="ProseMirror" role="textbox" contenteditable="true" style="width:440px;min-height:64px">[user -&gt; web1] legacy pending task</div>
      <button type="button" id="composer-submit-button" data-testid="send-button" aria-label="Send prompt">Send</button>
    </form>
    <section id="messages"></section>
  </main>
  <script>
    const prompt = document.querySelector('#prompt-textarea');
    document.querySelector('[data-testid="send-button"]').addEventListener('click', () => {
      globalThis.sendClicks = (globalThis.sendClicks || 0) + 1;
      globalThis.submitted = prompt.innerText || prompt.textContent || '';
      const message = document.createElement('article');
      message.setAttribute('data-message-author-role', 'user');
      message.textContent = globalThis.submitted;
      document.querySelector('#messages').appendChild(message);
      prompt.textContent = '';
      prompt.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'deleteContentBackward' }));
      history.pushState({}, '', '/c/test-conversation');
    });
  </script>
</body>
</html>"#
                }
                PromptPageBehavior::LegacyEdited => {
                    r#"<!doctype html>
<html>
<body>
  <main>
    <form id="composer-form">
      <div id="prompt-textarea" class="ProseMirror" role="textbox" contenteditable="true" style="width:440px;min-height:64px">user edited this draft</div>
      <button type="button" id="composer-submit-button" data-testid="send-button" aria-label="Send prompt">Send</button>
    </form>
  </main>
  <script>
    document.querySelector('[data-testid="send-button"]').addEventListener('click', () => {
      globalThis.sendClicks = (globalThis.sendClicks || 0) + 1;
    });
  </script>
</body>
</html>"#
                }
            };
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
