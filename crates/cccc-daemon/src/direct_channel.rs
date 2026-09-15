//! One bounded duplex TLS channel per approved Group pair; the durable outbox is shared.
use crate::dispatch_concurrency::DispatchLocks;
use cccc_contracts::{DaemonRequest, connect::ConnectPeerRequest, direct::*};
use cccc_core::{HomeLayout, direct, instance_identity::InstanceIdentity};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{Mutex, Semaphore, mpsc, oneshot},
    task::JoinSet,
};

const MAX_FRAME: usize = 14 * 1024 * 1024;
#[cfg(test)]
#[path = "direct_channel_tests.rs"]
mod tests;
type Job = (ConnectPeerRequest, oneshot::Sender<Result<Value, String>>);
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
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    id: String,
    secret: String,
    endpoint: DirectEndpoint,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Frame {
    Request { request: Box<ConnectPeerRequest> },
    Response { id: String, response: Value },
    Ping,
}
async fn read<T: DeserializeOwned>(
    stream: &mut (impl AsyncRead + Unpin),
    limit: usize,
) -> Result<T, String> {
    let length = stream
        .read_u32()
        .await
        .map_err(|_| "Direct channel closed")? as usize;
    if length == 0 || length > limit {
        return Err("Direct frame exceeds its limit".into());
    }
    let mut raw = vec![0; length];
    stream
        .read_exact(&mut raw)
        .await
        .map_err(|_| "Direct frame was interrupted")?;
    serde_json::from_slice(&raw).map_err(|_| "Invalid direct frame".into())
}
async fn write(
    stream: &mut (impl AsyncWrite + Unpin),
    value: &impl Serialize,
) -> Result<(), String> {
    let raw = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if raw.len() > MAX_FRAME {
        return Err("Direct frame exceeds its limit".into());
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        stream.write_u32(raw.len() as u32).await?;
        stream.write_all(&raw).await?;
        stream.flush().await
    })
    .await
    .map_err(|_| "Direct write timed out")?
    .map_err(|_| "Direct write was interrupted".into())
}

pub(crate) async fn run(home: HomeLayout, locks: DispatchLocks, channels: Channels) {
    let mut listener: Option<tokio::net::TcpListener> = None;
    let mut configured = None;
    let mut dialing = HashMap::new();
    let mut work = JoinSet::new();
    let mut errors = serde_json::Map::new();
    let permits = Arc::new(Semaphore::new(64));
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            Some(result)=work.join_next_with_id(), if !work.is_empty() => {
                let task=result.as_ref().map_or_else(|e|e.id(),|(id,_)|*id);
                dialing.retain(|_,(id,_)|*id!=task);
                if let Ok((_,(Some(id),result)))=result {match result {Ok(())=>{errors.remove(&id);},Err(error)=>{errors.insert(id,json!(error));}}}
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
                    if dialing.contains_key(&relation.id) { continue; }
                    let relation=relation.clone();let id=relation.id.clone();let home=home.clone();let locks=locks.clone();let channels=channels.clone();
                    let handle=work.spawn(async move {
                        let id=relation.id.clone();
                        // Only the receiver knows whether approval preceded expiry.
                        // Keep reconciliation possible, at a slower cadence after the deadline.
                        let result=dial(home.clone(),locks,channels,relation).await;
                        let delay=if direct::load(&home).is_ok_and(|store| store.relations.iter().any(|r|r.id==id && r.state==DirectState::Pending && !direct::unexpired(&r.expires_at))) {60} else {5};
                        // A bounded retry cadence avoids a failure loop; outbox retries are separate.
                        tokio::time::sleep(Duration::from_secs(delay)).await;
                        (Some(id),result)
                    });
                    dialing.insert(id,(handle.id(),handle));
                }
                dialing.retain(|id,(_,task)| { if keep.contains(id) {true} else {task.abort();false} });
                let online={let mut channels=channels.0.lock().await;channels.retain(|_,s|!s.is_closed());channels.keys().cloned().collect::<Vec<_>>()};
                errors.retain(|id,_|store.relations.iter().any(|r|&r.id==id));
                if store.listener.is_none() && store.relations.is_empty() {continue;}
                let status=json!({"checked_at":cccc_contracts::utc_now(),"listener":listener.is_some(),"bind":desired,"error":listener_error,"online":online,"errors":errors});
                let _=cccc_core::fs::write_secret_json(&home.root().join("state/connect/direct_status.json"),&status);
            }
        }
    }
}

async fn accept(
    home: HomeLayout,
    locks: DispatchLocks,
    channels: Channels,
    stream: tokio::net::TcpStream,
) -> Result<(), String> {
    let identity = InstanceIdentity::load(&home).map_err(|e| e.to_string())?;
    let acceptor = tokio_rustls::TlsAcceptor::from(
        crate::direct_tls::server(identity).map_err(|e| e.to_string())?,
    );
    let (stream, hello, state) = tokio::time::timeout(Duration::from_secs(10), async {
        let mut stream = acceptor
            .accept(stream)
            .await
            .map_err(|_| "Direct TLS handshake failed")?;
        if stream.get_ref().1.alpn_protocol() != Some(b"cccc-direct/1".as_slice()) {
            return Err("Unsupported direct protocol".into());
        }
        let cert = stream
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|c| c.first())
            .ok_or("Missing peer identity")?;
        let key = crate::direct_tls::public_key(cert.as_ref()).map_err(|e| e.to_string())?;
        let hello: Hello = read(&mut stream, 8192).await?;
        let state = direct::hello(&home, &hello.id, &hello.secret, &hello.endpoint, &key)
            .map_err(|e| e.to_string())?;
        write(&mut stream, &state).await?;
        Ok::<_, String>((stream, hello, state))
    })
    .await
    .map_err(|_| "Direct handshake timed out")??;
    if state != DirectState::Active {
        return Ok(());
    }
    channel(
        home,
        locks,
        channels,
        hello.id,
        hello.endpoint.instance_id,
        stream,
    )
    .await
}
async fn dial(
    home: HomeLayout,
    locks: DispatchLocks,
    channels: Channels,
    relation: DirectRelation,
) -> Result<(), String> {
    let invitation = relation
        .invitation
        .as_ref()
        .ok_or("Missing direct invitation")?;
    let identity = InstanceIdentity::load(&home).map_err(|e| e.to_string())?;
    let connector = tokio_rustls::TlsConnector::from(
        crate::direct_tls::client(identity, invitation.host.public_key.clone())
            .map_err(|e| e.to_string())?,
    );
    let (stream, state) = tokio::time::timeout(Duration::from_secs(10), async {
        let tcp = tokio::net::TcpStream::connect(&invitation.address)
            .await
            .map_err(|_| "Direct endpoint is unreachable")?;
        let name =
            rustls::pki_types::ServerName::try_from("direct.invalid").map_err(|e| e.to_string())?;
        let mut stream = connector
            .connect(name, tcp)
            .await
            .map_err(|_| "Direct TLS identity check failed")?;
        if stream.get_ref().1.alpn_protocol() != Some(b"cccc-direct/1".as_slice()) {
            return Err("Unsupported direct protocol".into());
        }
        write(
            &mut stream,
            &Hello {
                id: relation.id.clone(),
                secret: invitation.secret.clone(),
                endpoint: relation.local.clone(),
            },
        )
        .await?;
        let state: DirectState = read(&mut stream, 1024).await?;
        Ok::<_, String>((stream, state))
    })
    .await
    .map_err(|_| "Direct connection timed out")??;
    direct::update(&home, |store| {
        let current = store
            .relations
            .iter_mut()
            .find(|r| r.id == relation.id)
            .ok_or_else(|| std::io::Error::other("Connection was removed"))?;
        if matches!(current.state, DirectState::Revoked | DirectState::Expired) {
            return Err(std::io::Error::other("Connection was revoked"));
        }
        if matches!(
            state,
            DirectState::Active | DirectState::Revoked | DirectState::Expired
        ) {
            current.state = state.clone();
        }
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    if state != DirectState::Active {
        return Ok(());
    }
    channel(
        home,
        locks,
        channels,
        relation.id,
        invitation.host.instance_id.clone(),
        stream,
    )
    .await
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
            match frame {
                Ok(Ok(frame)) => {
                    if incoming_tx.send(frame).await.is_err() {
                        break;
                    }
                }
                _ => break,
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
                    let Some(frame)=frame else {break;};
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
