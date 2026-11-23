//! One bounded duplex TLS channel per approved Group pair; the durable outbox is shared.
use crate::dispatch_concurrency::DispatchLocks;
use cccc_contracts::{DaemonRequest, connect::ConnectPeerRequest, direct::*};
use cccc_core::{HomeLayout, direct, instance_identity::InstanceIdentity};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{Mutex, Semaphore, mpsc, oneshot},
    task::JoinSet,
    time::Instant,
};

#[path = "direct_channel_retry.rs"]
mod retry;
use retry::RetryState;

#[path = "direct_channel_frames.rs"]
mod frames;
use frames::{Frame, MAX_FRAME, read, write};
#[path = "direct_channel_handshake.rs"]
mod handshake;
use handshake::{accept, dial};
#[cfg(test)]
#[path = "direct_channel_lifecycle_tests.rs"]
mod lifecycle_tests;
#[cfg(test)]
#[path = "direct_channel_shared_tests.rs"]
mod shared_tests;
#[cfg(test)]
#[path = "direct_channel_tests.rs"]
mod tests;
type Job = (ConnectPeerRequest, oneshot::Sender<Result<Value, String>>);
type ConnectionTaskResult = (Option<(String, RetryState)>, Result<(), String>);
#[derive(Clone, Default)]
pub(crate) struct Channels(Arc<Mutex<HashMap<String, mpsc::Sender<Job>>>>);
impl Channels {
    pub async fn exchange(&self, request: ConnectPeerRequest) -> Result<Value, String> {
        let id = request
            .proof
            .connection_id
            .as_deref()
            .ok_or("missing direct connection")?;
        let sender = self
            .0
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or("Direct peer is offline")?;
        let (tx, rx) = oneshot::channel();
        sender
            .try_send((request, tx))
            .map_err(|_| "Direct channel is busy or closed")?;
        tokio::time::timeout(Duration::from_secs(10), rx)
            .await
            .map_err(|_| "Direct request timed out")?
            .map_err(|_| "Direct channel closed")?
    }
}
pub(crate) async fn run(home: HomeLayout, locks: DispatchLocks, channels: Channels) {
    let mut listener: Option<tokio::net::TcpListener> = None;
    let mut configured = None;
    let mut dialing = HashMap::new();
    let mut retries: HashMap<String, RetryState> = HashMap::new();
    let mut work: JoinSet<ConnectionTaskResult> = JoinSet::new();
    let mut errors = serde_json::Map::new();
    let permits = Arc::new(Semaphore::new(64));
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            Some(result)=work.join_next_with_id(), if !work.is_empty() => {
                let task=result.as_ref().map_or_else(|e|e.id(),|(id,_)|*id);
                dialing.retain(|_,(id,_)|*id!=task);
                if let Ok((_,(Some((id,mut retry)),result)))=result {
                    let awaiting_expired=direct::load(&home).is_ok_and(|store|store.relations.iter().any(|r|r.id==id && r.state==DirectState::Pending && !direct::unexpired(&r.expires_at)));
                    retry.completed(Instant::now(),result.is_ok(),awaiting_expired);
                    retries.insert(id.clone(),retry);
                    match result {Ok(())=>{errors.remove(&id);},Err(error)=>{errors.insert(id,json!(error));}}
                }
            }
            incoming=async { match &listener { Some(listener)=>listener.accept().await, None=>std::future::pending().await } } => {
                if let Ok((stream,_))=incoming && let Ok(permit)=permits.clone().try_acquire_owned() {
                    let home=home.clone(); let locks=locks.clone(); let channels=channels.clone();
                    work.spawn(async move { let _permit=permit; (None,accept(home,locks,channels,stream).await) });
                }
            }
            _=tick.tick()=> {
                let Ok(store)=direct::load(&home) else { continue; };
                let desired=store.listener.as_ref().map(|l|l.bind.clone());
                if desired!=configured { listener=None; configured=desired.clone(); }
                let mut listener_error=None;
                if listener.is_none() && let Some(bind)=&desired {
                    match tokio::net::TcpListener::bind(bind).await {
                        Ok(bound)=>listener=Some(bound), Err(e)=>listener_error=Some(e.to_string()),
                    }
                }
                let mut keep=std::collections::HashSet::new();
                for relation in &store.relations {
                    if relation.invitation.is_none() || !matches!(relation.state, DirectState::Pending | DirectState::Active) || !direct::current(&home,&relation.local).unwrap_or(false) { continue; }
                    keep.insert(relation.id.clone());
                    if dialing.contains_key(&relation.id) || retries.get(&relation.id).is_some_and(|retry|!retry.ready(Instant::now())) { continue; }
                    let relation=relation.clone();let id=relation.id.clone();let home=home.clone();let locks=locks.clone();let channels=channels.clone();
                    let mut retry=retries.remove(&id).unwrap_or_default();
                    let handle=work.spawn(async move {
                        let id=relation.id.clone();
                        let result=dial(home,locks,channels,relation,&mut retry).await;
                        (Some((id,retry)),result)
                    });
                    dialing.insert(id,(handle.id(),handle));
                }
                dialing.retain(|id,(_,task)| { if keep.contains(id) {true} else {task.abort();false} });
                retries.retain(|id,_|keep.contains(id));
                let online={let mut channels=channels.0.lock().await;channels.retain(|_,s|!s.is_closed());channels.keys().cloned().collect::<Vec<_>>()};
                errors.retain(|id,_|store.relations.iter().any(|r|&r.id==id));
                if store.listener.is_none() && store.relations.is_empty() {continue;}
                let status=json!({"checked_at":cccc_contracts::utc_now(),"listener":listener.is_some(),"bind":desired,"error":listener_error,"online":online,"errors":errors});
                let _=cccc_core::fs::write_secret_json(&home.root().join("state/connect/direct_status.json"),&status);
            }
        }
    }
}

async fn channel<S>(
    home: HomeLayout,
    locks: DispatchLocks,
    channels: Channels,
    id: String,
    peer: String,
    stream: S,
) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (sender, mut jobs) = mpsc::channel::<Job>(4);
    {
        let mut all = channels.0.lock().await;
        if all.get(&id).is_some_and(|sender| !sender.is_closed()) {
            return Err("Direct channel already connected".into());
        }
        all.insert(id.clone(), sender.clone());
    }
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (incoming_tx, mut incoming) = mpsc::channel(2);
    let mut tasks = JoinSet::new();
    tasks.spawn(async move {
        loop {
            let frame = tokio::time::timeout(
                Duration::from_secs(45),
                read::<Frame>(&mut reader, MAX_FRAME),
            )
            .await;
            let frame = frame.unwrap_or_else(|_| Err("Direct peer heartbeat timed out".into()));
            let failed = frame.is_err();
            if incoming_tx.send(frame).await.is_err() || failed {
                break;
            }
        }
    });
    let mut handlers = JoinSet::new();
    let mut pending: HashMap<String, oneshot::Sender<Result<Value, String>>> = HashMap::new();
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    let mut pulse = 0;
    let result=async {
        loop {
            tokio::select! {
                _=tick.tick()=> {
                    direct::binding(&home,&peer,&id)?;
                    let store=direct::load(&home).map_err(|e|e.to_string())?;
                    if store.listener.is_none() && store.relations.iter().any(|r|r.id==id && r.invitation.is_none()) {return Err("Direct listener is disabled".into());}
                    pending.retain(|_,reply|!reply.is_closed());
                    pulse+=1; if pulse%5==0 {write(&mut writer,&Frame::Ping).await?;}
                }
                job=jobs.recv()=> {
                    let Some((request,reply))=job else {break;};
                    if pending.len()>=4 { let _=reply.send(Err("Direct channel is busy".into())); continue; }
                    direct::binding(&home,&peer,&id)?;
                    let request_id=request.proof.request_id.clone();
                    write(&mut writer,&Frame::Request{request:Box::new(request)}).await?;
                    pending.insert(request_id,reply);
                }
                Some(result)=handlers.join_next(), if !handlers.is_empty()=> {
                    let (request_id,response)=result.map_err(|_|"Direct operation worker failed")?;
                    write(&mut writer,&Frame::Response{id:request_id,response}).await?;
                }
                frame=incoming.recv()=> {
                    let frame=frame.ok_or("Direct reader stopped")??;
                    match frame {
                        Frame::Ping=>{},
                        Frame::Response{id,response}=>{if let Some(reply)=pending.remove(&id) {let _=reply.send(Ok(response));}},
                        Frame::Request{request}=> {
                            if request.proof.source_instance_id!=peer || request.proof.connection_id.as_deref()!=Some(id.as_str()) {return Err("Direct request is outside its channel".into());}
                            cccc_core::connect_peer::authenticate(&home,&request)?;
                            if handlers.len()>=4 {return Err("Direct peer exceeded operation concurrency".into());}
                            let home=home.clone();let locks=locks.clone();
                            handlers.spawn(async move {
                                let id=request.proof.request_id.clone();
                                let args=json!({"group_id":request.operation.target_group_id(),"envelope":request});
                                let request=DaemonRequest{v:1,op:"connect_peer_receive".into(),args:args.as_object().expect("object").clone()};
                                let permit=locks.acquire(&request).await;
                                let response=tokio::task::spawn_blocking(move || {let _permit=permit;crate::dispatch::dispatch(&home,&request)}).await;
                                (id,response.ok().and_then(|r|serde_json::to_value(r).ok()).unwrap_or(Value::Null))
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }.await;
    let mut all = channels.0.lock().await;
    if all
        .get(&id)
        .is_some_and(|current| current.same_channel(&sender))
    {
        all.remove(&id);
    }
    result
}
