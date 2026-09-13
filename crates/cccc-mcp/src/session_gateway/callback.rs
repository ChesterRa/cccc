//! First-time local return target selection; reads tab metadata only, never page contents.
use crate::ToolCallError;
use cccc_core::{HomeLayout, web_model_connectors as store};
use serde_json::{Value, json};

pub(super) async fn finish(
    home: &HomeLayout,
    session: &str,
    payload: Value,
    capture: bool,
) -> Result<Value, ToolCallError> {
    finish_with(home, session, payload, capture, select_local_chat).await
}

async fn finish_with<F, Fut>(
    home: &HomeLayout,
    session: &str,
    mut payload: Value,
    capture: bool,
    select: F,
) -> Result<Value, ToolCallError>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Value>,
{
    let binding = store::find_session(home, session)
        .map_err(super::binding_error)?
        .ok_or_else(super::required_binding)?;
    let group = binding["group_id"]
        .as_str()
        .ok_or_else(super::required_binding)?;
    let actor = binding["actor_id"]
        .as_str()
        .ok_or_else(super::required_binding)?;
    if payload["group_id"] != group || payload["actor_id"] != actor {
        return Err(super::required_binding());
    }
    let (target, owner) =
        store::browser_target_snapshot(home, group, actor).map_err(super::binding_error)?;
    // The session may have been replaced between route lookup and this atomic snapshot.
    if !owner.belongs_to_session(session) {
        return Err(super::required_binding());
    }
    let saved = (target["kind"] == "existing_chat")
        .then(|| {
            target["url"]
                .as_str()
                .and_then(store::normalized_chatgpt_conversation_url)
        })
        .flatten();
    payload["callback_target_ready"] = json!(saved.is_some());
    if let Some(url) = saved {
        payload["status"] = json!("configured");
        payload["callback_url"] = json!(url);
        payload["callback_capture"] = json!({"status":"saved"});
        return Ok(payload);
    }
    if !capture {
        return Ok(payload);
    }
    // Do not replace explicit settings, including the existing new-chat mode.
    if target.as_object().is_some_and(|value| !value.is_empty()) {
        payload["callback_capture"] = json!({"status":"existing_target_preserved"});
        return Ok(payload);
    }
    let selection = select(payload["group_title"].as_str().unwrap_or(group).to_owned()).await;
    let url = (selection["status"] == "confirmed")
        .then(|| {
            selection["url"]
                .as_str()
                .and_then(store::normalized_chatgpt_conversation_url)
        })
        .flatten();
    if let Some(url) = url {
        let target = json!({"kind":"existing_chat","state":"bound_existing_chat","url":url,
            "saved_at":cccc_contracts::utc_now(),"next_delivery":"existing_chat"});
        let home = home.clone();
        let (gid, aid) = (group.to_owned(), actor.to_owned());
        let saved = tokio::task::spawn_blocking(move || {
            store::save_browser_target_if_current(&home, &gid, &aid, target, &owner)
        })
        .await
        .map_err(|e| super::binding_error(std::io::Error::other(e)))?
        .map_err(super::binding_error)?;
        if !saved {
            return Err(
                "callback_capture_stale: binding or target changed; nothing was overwritten".into(),
            );
        }
        payload["callback_target_ready"] = json!(true);
        payload["callback_url"] = json!(url);
        payload["status"] = json!("configured");
        payload["next_steps"] = json!([
            "Return URL saved. Call cccc_bootstrap, then dispatch normally without providing a URL again. Browser login and an actual return delivery still need verification."
        ]);
    } else {
        payload["next_steps"] = json!([
            "Return address is not saved; local dispatch still works. Do not guess a URL or retry automatically. The user can open this same chat on the Mac and retry cccc_group_bind for a one-time local selection, or provide its URL explicitly."
        ]);
    }
    payload["callback_capture"] = json!({"status":selection["status"]});
    Ok(payload)
}

async fn select_local_chat(group: String) -> Value {
    #[cfg(target_os = "macos")]
    {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(45),
            tokio::process::Command::new("/usr/bin/osascript")
                .args(["-l", "JavaScript", "-e", SELECT_CHAT, "--", &group])
                .kill_on_drop(true)
                .output(),
        )
        .await;
        match result {
            Ok(Ok(output)) if output.status.success() => serde_json::from_slice(&output.stdout)
                .unwrap_or_else(|_| json!({"status":"unavailable"})),
            Err(_) => json!({"status":"selection_timeout"}),
            _ => json!({"status":"unavailable"}),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = group;
        json!({"status":"unsupported_platform"})
    }
}

// Native browser metadata and standard OS selection only. No page JavaScript or permission edits.
#[cfg(any(target_os = "macos", test))]
const SELECT_CHAT: &str = r#"function run(argv) {
  const stable = raw => {
    const url = String(raw || '').split(/[?#]/)[0];
    return /^https:\/\/chatgpt\.com\/(?:g\/[^/]+\/)?c\/[a-zA-Z0-9-]+$/.test(url) ? url : '';
  };
  const tabs = [];
  for (const name of ['Microsoft Edge', 'Google Chrome']) {
    try {
      const browser = Application(name);
      if (!browser.running()) continue;
      for (const win of browser.windows()) for (const tab of win.tabs()) {
        const url = stable(tab.url());
        if (url && !tabs.some(item => item.url === url)) tabs.push({tab, url,
          label:name + ' · ' + String(tab.title()).replace(/[\r\n\t]/g, ' ') + '\n' + url});
      }
    } catch (_) { /* Absent browser or denied automation is not a candidate. */ }
  }
  if (!tabs.length) return JSON.stringify({status:'not_found'});
  const labels = tabs.map((item, i) => String(i + 1) + '. ' + item.label);
  const app = Application.currentApplication();
  app.includeStandardAdditions = true;
  const choice = app.chooseFromList(labels, {
    withTitle:'CCCC · 绑定回传聊天',
    withPrompt:'工作组：' + argv[0] + '\n请选择正在调用 CCCC 的这条聊天。保存后，成员报告会自动回到这里。',
    okButtonName:'绑定此聊天', cancelButtonName:'取消',
    multipleSelectionsAllowed:false, emptySelectionAllowed:false
  });
  if (!choice || choice.length !== 1) return JSON.stringify({status:'cancelled'});
  const item = tabs[labels.indexOf(String(choice[0]))];
  try { if (item && stable(item.tab.url()) === item.url) return JSON.stringify({status:'confirmed',url:item.url}); }
  catch (_) { /* A closed or navigated tab cannot be saved. */ }
  return JSON.stringify({status:'tab_changed'});
}"#;

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::{Actor, ActorRuntime};
    use cccc_core::{GroupStore, actors};

    #[tokio::test]
    async fn first_capture_preserves_cancellation_explicit_targets_and_changed_ownership() {
        for case in [
            "save",
            "cancelled",
            "not_found",
            "selection_timeout",
            "unavailable",
            "invalid_url",
            "target_changed",
            "rebound",
            "revoked",
            "disabled",
            "existing",
            "new_chat",
            "opt_out",
        ] {
            let temp = tempfile::tempdir().expect("isolated callback");
            let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
            let groups = GroupStore::new(home.clone()).expect("groups");
            let mut group = groups.create("capture", "").expect("group");
            let mut web = Actor::new("web");
            web.runtime = ActorRuntime::WebModel;
            actors::add(&mut group, web).expect("web member");
            groups.save(&group).expect("save");
            let gid = &group.group_id;
            let (connector, _) = store::create(&home, gid, "web", "chatgpt", "web").expect("route");
            let cid = connector["connector_id"].as_str().expect("connector");
            let bind = |session: &str| {
                let code = store::prepare_binding(&home, cid, 600).expect("code");
                store::bind_session(
                    &home,
                    cid,
                    code["code"].as_str().expect("issued binding code"),
                    session,
                )
                .expect("bind");
            };
            bind("this-chat");
            if case == "save" {
                let (_, owner) = store::browser_target_snapshot(&home, gid, "web").expect("owner");
                assert!(owner.belongs_to_session("this-chat"));
                assert!(!owner.belongs_to_session("other-chat"));
            }
            let manual = json!({"kind":"existing_chat","url":"https://chatgpt.com/c/manual"});
            if matches!(case, "existing" | "new_chat") {
                let target = if case == "existing" {
                    manual.clone()
                } else {
                    json!({"kind":"new_chat","url":"https://chatgpt.com/"})
                };
                store::save_browser_target(&home, gid, "web", Some(target))
                    .expect("explicit selection");
            }
            let calls = std::cell::Cell::new(0);
            let payload = json!({"group_id":gid,"actor_id":"web","status":"needs_chat_url","can_dispatch":true});
            let outcome = finish_with(&home, "this-chat", payload, case != "opt_out", |_| async {
                calls.set(calls.get() + 1);
                match case {
                    "target_changed" => store::save_browser_target(&home, gid, "web", Some(manual.clone())).expect("manual selection during picker"),
                    "rebound" => bind("replacement-chat"),
                    "revoked" => { store::revoke(&home, cid).expect("revoke during picker"); }
                    "disabled" => { groups.mutate(gid, |g| {g.actors[0].enabled = false; Ok(())}).expect("disable during picker"); }
                    _ => {}
                }
                if matches!(case, "cancelled" | "not_found" | "selection_timeout" | "unavailable") {
                    json!({"status":case})
                } else {
                    json!({"status":"confirmed","url":if case == "invalid_url" {"https://example.com/c/wrong"} else {"https://chatgpt.com/c/selected?noise=1#view"}})
                }
            }).await;
            assert_eq!(
                calls.get(),
                usize::from(!matches!(case, "existing" | "new_chat" | "opt_out")),
                "{case}"
            );
            assert_eq!(
                outcome.is_err(),
                matches!(case, "target_changed" | "rebound" | "revoked" | "disabled"),
                "{case}: {outcome:?}"
            );
            let target = store::browser_target(&home, gid, "web").expect("persisted target");
            if case == "save" {
                assert_eq!(
                    outcome.expect("confirmed callback saved")["callback_target_ready"],
                    true
                );
                assert_eq!(target["url"], "https://chatgpt.com/c/selected");
                let reopened = HomeLayout::from_path(home.root().to_owned()).expect("restart");
                let retry = finish_with(
                    &reopened,
                    "this-chat",
                    json!({"group_id":gid,"actor_id":"web"}),
                    true,
                    |_| async { panic!("saved callback must not read tabs or prompt again") },
                )
                .await
                .expect("reuse after restart");
                assert_eq!(retry["callback_target_ready"], true);
            } else if matches!(case, "existing" | "target_changed") {
                assert_eq!(target["url"], manual["url"]);
            } else if case == "new_chat" {
                assert_eq!(target["kind"], "new_chat");
            } else {
                assert!(
                    target.as_object().expect("target").is_empty(),
                    "{case}: {target}"
                );
            }
        }
    }

    #[test]
    fn native_picker_filters_addresses_requires_choice_and_rechecks_navigation() {
        let harness = r#"
import assert from 'node:assert/strict';
let rows = [], choice = -1, prompts = 0, changed = false;
function Application(name) {
  return {running:()=>name==='Microsoft Edge', windows:()=>[{tabs:()=>rows.map(row=>({
    url:()=>row.url, title:()=>row.title || 'chat', execute:()=>{throw Error('page scripts forbidden');}
  }))}]};
}
Application.currentApplication = () => ({chooseFromList:(labels, options)=>{
  prompts++; assert.equal(options.defaultItems, undefined);
  assert.equal(options.multipleSelectionsAllowed, false);
  if (changed) rows[0].url = 'https://chatgpt.com/c/moved';
  return choice < 0 ? false : [labels[choice]];
}});
const check = (urls, selected, state, expected, move=false) => {
  rows = urls.map(url=>({url})); choice=selected; prompts=0; changed=move;
  const result = JSON.parse(run(['group'])); assert.equal(result.status,state);
  if(expected) assert.equal(result.url,expected);
  return prompts;
};
assert.equal(check([],0,'not_found'),0);
assert.equal(check(['https://example.com/c/a','https://chatgpt.com/c/WEB:pending','https://chatgpt.com/'],0,'not_found'),0);
assert.equal(check(['https://chatgpt.com/c/a'],-1,'cancelled'),1);
check(['https://chatgpt.com/c/a','https://chatgpt.com/c/b'],1,'confirmed','https://chatgpt.com/c/b');
check(['https://chatgpt.com/c/a?x=1','https://chatgpt.com/c/a#view'],0,'confirmed','https://chatgpt.com/c/a');
check(['https://chatgpt.com/g/g-test/c/a'],0,'confirmed','https://chatgpt.com/g/g-test/c/a');
check(['https://chatgpt.com/c/a'],0,'tab_changed',null,true);
console.log('native picker: seven isolated scenarios passed; no real browser or dialog');
"#;
        let result = std::process::Command::new("node")
            .args([
                "--input-type=module",
                "-e",
                &format!("{SELECT_CHAT}\n{harness}"),
            ])
            .output()
            .expect("existing Node runtime");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
