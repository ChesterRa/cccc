use super::super::browser_surface_tests;
use super::super::chrome_test_guard;
use super::*;

#[tokio::test]
async fn grok_controls_busy_state_drafts_and_user_receipts() {
    if crate::system_browser_path().is_none() {
        return;
    }
    let _chrome_guard = chrome_test_guard().await;
    let (url, server) = browser_surface_tests::local_page(r#"<!doctype html>
        <main><section data-testid="bot-transcript-scroller" role="region"></section>
        <div data-testid="bot-working-slot"></div>
        <section><div><div><div data-testid="chat-input"><div role="textbox" contenteditable="true" class="ProseMirror" style="width:400px;min-height:100px"></div></div></div></div>
        <input type="file"><button data-testid="attach-button">Attach</button><button data-testid="chat-submit" aria-label="Submit">↑</button></section></main>
        <script>window.sends=0;document.querySelector('[data-testid=chat-submit]').onclick=()=>{window.sends++;const editor=document.querySelector('[contenteditable]'); const receipt=document.createElement('div');receipt.dataset.testid='user-message';receipt.setAttribute('role','article');receipt.textContent=editor.innerText;document.querySelector('[data-testid=bot-transcript-scroller]').append(receipt);editor.textContent='';};</script>"#).await;
    let temp = tempfile::tempdir().expect("valid test fixture");
    let manager = BrowserSurfaces::default();
    manager
        .open("grok", &temp.path().join("profile"), &url, 900, 650)
        .await
        .expect("valid test fixture");
    let page = manager.page("grok").await.expect("valid test fixture");
    let url = page
        .url()
        .await
        .expect("valid test fixture")
        .expect("valid test fixture");
    let prompt = "[cccc] Browser batch webdelivery:A:grok events=e1 actor=A\nTask";
    let needles = vec!["[cccc] Browser batch webdelivery:A:grok events=e1 actor=A".into()];
    wait_for_composer(&page).await.expect("valid test fixture");
    page.evaluate(format!(
        "document.querySelector('[contenteditable]').textContent={}",
        json!(prompt)
    ))
    .await
    .expect("valid test fixture");
    let ready = inspect_send_control(&page)
        .await
        .expect("valid test fixture");
    assert_eq!(ready.descriptor, "[data-testid=chat-submit]");
    assert!(!ready.running);
    // Bot can keep Submit enabled while running: the working slot still fences it.
    page.evaluate("document.querySelector('[data-testid=bot-working-slot]').innerHTML='<div style=\"width:100px;height:30px\">Working</div>'").await.expect("valid test fixture");
    assert_eq!(
        manager
            .prompt_readiness("grok")
            .await
            .expect("valid test fixture")["running"],
        true
    );
    assert!(
        inspect_send_control(&page)
            .await
            .expect("valid test fixture")
            .selector
            .is_empty()
    );
    assert!(
        !activate_send_control(&page, &ready, prompt, &needles, &url)
            .await
            .expect("valid test fixture")
            .invoked
    );
    page.evaluate("document.querySelector('[data-testid=bot-working-slot]').textContent=''")
        .await
        .expect("valid test fixture");
    // An assistant echo must not settle the batch.
    page.evaluate(format!("document.querySelector('[data-testid=bot-transcript-scroller]').innerHTML='<div data-testid=assistant-message></div>';document.querySelector('[data-testid=assistant-message]').textContent={}",json!(prompt))).await.expect("valid test fixture");
    assert!(
        !inspect_submission(&page, prompt, &needles)
            .await
            .expect("valid test fixture")
            .echo_found
    );
    let ready = inspect_send_control(&page)
        .await
        .expect("valid test fixture");
    assert!(
        activate_send_control(&page, &ready, prompt, &needles, &url)
            .await
            .expect("valid test fixture")
            .invoked
    );
    assert!(
        inspect_submission(&page, prompt, &needles)
            .await
            .expect("valid test fixture")
            .echo_found
    );
    assert!(
        !activate_send_control(&page, &ready, prompt, &needles, &url)
            .await
            .expect("valid test fixture")
            .invoked
    );
    assert_eq!(
        page.evaluate("window.sends")
            .await
            .expect("valid test fixture")
            .into_value::<u32>()
            .expect("valid test fixture"),
        1
    );
    page.evaluate("const dt=new DataTransfer();dt.items.add(new File(['data'],'human.txt'));document.querySelector('input').files=dt.files;").await.expect("valid test fixture");
    assert!(
        composer_has_attachments(&page)
            .await
            .expect("valid test fixture")
    );
    manager.close("grok").await.expect("valid test fixture");
    server.abort();
}
