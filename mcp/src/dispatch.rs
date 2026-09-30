//! Eşzamanlı istek dağıtımı: her istek kendi görevinde çalışır, stdout'u tek bir
//! yazıcı görev sahiplenir ve `notifications/cancelled` süren isteği durdurur.

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, Semaphore};
use tokio::task::{AbortHandle, JoinError, JoinHandle, JoinSet};

use crate::protocol::{create_error_response, JsonRpcRequest, JsonRpcResponse};
use crate::server::{self, ServerState};

/// Aynı anda çalışan en fazla araç çağrısı; fazlası izin bekler, reddedilmez.
const MAX_CONCURRENT_TOOL_CALLS: usize = 16;
/// Yazıcı kuyruğunun kapasitesi; stdout tıkanırsa gönderenler bekler.
const OUTBOX_CAPACITY: usize = 64;

/// Satır sonu olmadan serileştirilmiş tek bir JSON-RPC mesajı.
struct OutgoingFrame(String);

/// Stdout yazıcısına mesaj gönderen uç. Her mesaj tek parça bir satır olarak
/// yazılır; eşzamanlı görevlerin çıktıları iç içe geçmez.
#[derive(Clone)]
pub(crate) struct Outbox {
    frames: mpsc::Sender<OutgoingFrame>,
}

impl Outbox {
    pub(crate) async fn send<M: Serialize>(&self, message: &M) -> Result<()> {
        let frame =
            serde_json::to_string(message).context("JSON-RPC message could not be serialized")?;
        self.frames
            .send(OutgoingFrame(frame))
            .await
            .map_err(|_| anyhow::anyhow!("stdout writer has stopped; the message was not sent"))
    }
}

/// Stdout'un tek sahibi olan yazıcı görevi başlatır. Görev, bütün `Outbox`
/// kopyaları bırakılıp kuyruk boşalınca biter; yazma hatasında hatayla biter.
pub(crate) fn start_writer<W>(writer: W, debug: bool) -> (Outbox, JoinHandle<Result<()>>)
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (frames, receiver) = mpsc::channel(OUTBOX_CAPACITY);
    let task = tokio::spawn(run_writer(writer, receiver, debug));
    (Outbox { frames }, task)
}

async fn run_writer<W>(
    mut writer: W,
    mut frames: mpsc::Receiver<OutgoingFrame>,
    debug: bool,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    while let Some(OutgoingFrame(frame)) = frames.recv().await {
        if debug {
            tracing::debug!(payload = %crate::sanitize_payload(&frame), "Sending JSON-RPC message");
        }
        writer
            .write_all(frame.as_bytes())
            .await
            .context("writing a JSON-RPC message to stdout failed")?;
        writer
            .write_all(b"\n")
            .await
            .context("writing a JSON-RPC message to stdout failed")?;
        writer.flush().await.context("flushing stdout failed")?;
    }
    Ok(())
}

/// Yazıcı görevin bitiş sonucunu oturum sonucuna çevirir.
pub(crate) fn writer_result(joined: std::result::Result<Result<()>, JoinError>) -> Result<()> {
    joined.context("stdout writer task failed")?
}

/// İstek kimliğinin kanonik JSON metni; `1` ile `"1"` farklı isteklerdir.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RequestKey(String);

impl RequestKey {
    /// Kimliği olmayan mesaj JSON-RPC'deki gibi `null` kimlikle anahtarlanır.
    fn new(id: Option<&Value>) -> Self {
        match id {
            Some(value) => Self(value.to_string()),
            None => Self(Value::Null.to_string()),
        }
    }
}

impl std::fmt::Display for RequestKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

struct InFlightEntry {
    /// Aynı kimliği yeniden kullanan istekleri ayırt eden dağıtım sırası.
    sequence: u64,
    abort: AbortHandle,
}

/// Süren isteklerin iptal tutamakları.
struct InFlightRequests {
    entries: std::sync::Mutex<HashMap<RequestKey, InFlightEntry>>,
}

impl InFlightRequests {
    fn new() -> Self {
        Self {
            entries: std::sync::Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<RequestKey, InFlightEntry>> {
        self.entries
            .lock()
            .expect("in-flight request registry lock poisoned")
    }

    fn register(&self, key: RequestKey, sequence: u64, abort: AbortHandle) {
        let previous = self
            .lock()
            .insert(key.clone(), InFlightEntry { sequence, abort });
        if previous.is_some() {
            tracing::warn!(
                request_id = %key,
                "Request id reused while an earlier request with it is still running; cancellation targets the newest request"
            );
        }
    }

    /// Biten isteğin kaydını siler; kimliği yeniden kullanan yeni isteğe dokunmaz.
    fn finish(&self, key: &RequestKey, sequence: u64) {
        let mut entries = self.lock();
        if entries
            .get(key)
            .is_some_and(|entry| entry.sequence == sequence)
        {
            entries.remove(key);
        }
    }

    /// Süren isteği durdurur; istek bilinmiyorsa ya da bittiyse `false`.
    fn cancel(&self, key: &RequestKey) -> bool {
        match self.lock().remove(key) {
            Some(entry) => {
                entry.abort.abort();
                true
            }
            None => false,
        }
    }
}

/// Dağıtılan bir isteğin yanıtı teslim etmek için gereken bilgileri.
struct DispatchedRequest {
    key: RequestKey,
    sequence: u64,
    id: Option<Value>,
    method: String,
    started: std::time::Instant,
}

/// İstekleri kendi görevlerinde çalıştırır ve iptal edilebilir tutar. Yalnızca
/// okuma döngüsü kullanır; kayıt ve iptal böylece mesaj sırasıyla işlenir.
pub(crate) struct Dispatcher {
    state: Arc<ServerState>,
    outbox: Outbox,
    in_flight: Arc<InFlightRequests>,
    tool_permits: Arc<Semaphore>,
    deliveries: JoinSet<()>,
    next_sequence: u64,
}

impl Dispatcher {
    pub(crate) fn new(state: Arc<ServerState>, outbox: Outbox) -> Self {
        Self {
            state,
            outbox,
            in_flight: Arc::new(InFlightRequests::new()),
            tool_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_TOOL_CALLS)),
            deliveries: JoinSet::new(),
            next_sequence: 0,
        }
    }

    /// İsteği kendi görevinde başlatır ve iptal için kaydeder. Yanıtı, işi
    /// bekleyen teslim görevi yazıcıya gönderir; iptal edilen istek yanıtsız kalır.
    pub(crate) fn dispatch(&mut self, request: JsonRpcRequest) {
        self.reap_deliveries();
        self.next_sequence += 1;
        let dispatched = DispatchedRequest {
            key: RequestKey::new(request.id.as_ref()),
            sequence: self.next_sequence,
            id: request.id.clone(),
            method: request.method.clone(),
            started: std::time::Instant::now(),
        };
        tracing::debug!(request_id = %dispatched.key, method = %dispatched.method, "Dispatching request");
        let worker = tokio::spawn(run_request(
            self.state.clone(),
            self.tool_permits.clone(),
            request,
        ));
        // Kayıt, teslim görevi başlamadan yapılır; biten işin kaydı böylece
        // her zaman kayıttan sonra silinir.
        self.in_flight.register(
            dispatched.key.clone(),
            dispatched.sequence,
            worker.abort_handle(),
        );
        self.deliveries.spawn(deliver_response(
            worker,
            dispatched,
            self.in_flight.clone(),
            self.outbox.clone(),
        ));
    }

    /// `notifications/cancelled`: süren isteği durdurur ve yanıtını göndermez.
    /// Bilinmeyen ya da bitmiş kimlikte iptal edilecek bir şey yoktur.
    pub(crate) fn cancel(&self, params: Option<&Value>) {
        let Some(request_id) = params.and_then(|params| params.get("requestId")) else {
            tracing::debug!(params = ?params, "Ignoring a cancellation without a requestId");
            return;
        };
        let key = RequestKey::new(Some(request_id));
        let reason = params
            .and_then(|params| params.get("reason"))
            .and_then(Value::as_str);
        if self.in_flight.cancel(&key) {
            tracing::info!(
                request_id = %key,
                reason = ?reason,
                "Cancelled an in-flight request; no response will be sent"
            );
        } else {
            tracing::debug!(
                request_id = %key,
                reason = ?reason,
                "Cancellation for an unknown or finished request; nothing to cancel"
            );
        }
    }

    /// Girdi kapandığında süren isteklerin bitmesini ve yanıtlarının yazıcı
    /// kuyruğuna girmesini bekler.
    pub(crate) async fn drain(mut self) {
        while let Some(joined) = self.deliveries.join_next().await {
            log_delivery_failure(joined);
        }
    }

    /// Bitmiş teslim görevlerini toplar; aksi halde sonuçları birikir.
    fn reap_deliveries(&mut self) {
        while let Some(joined) = self.deliveries.try_join_next() {
            log_delivery_failure(joined);
        }
    }
}

/// İsteği işler. Araç çağrıları eşzamanlılık izni bekler; protokol istekleri
/// (ping, tools/list gibi) beklemez, böylece uzun araç çağrıları canlılık
/// denetimlerini geciktirmez.
async fn run_request(
    state: Arc<ServerState>,
    tool_permits: Arc<Semaphore>,
    request: JsonRpcRequest,
) -> JsonRpcResponse {
    if request.method != "tools/call" {
        return server::handle_request(&state, request).await;
    }
    let permit = match tool_permits.acquire_owned().await {
        Ok(permit) => permit,
        Err(error) => {
            return create_error_response(
                request.id,
                -32603,
                &format!("Tool call could not be scheduled: {error}"),
            );
        }
    };
    let response = server::handle_request(&state, request).await;
    drop(permit);
    response
}

/// İşin sonucunu bekler, iptal kaydını siler ve yanıtı yazıcıya gönderir.
/// İptal edilen iş yanıt üretmez; çöken iş iç hata yanıtı alır.
async fn deliver_response(
    worker: JoinHandle<JsonRpcResponse>,
    request: DispatchedRequest,
    in_flight: Arc<InFlightRequests>,
    outbox: Outbox,
) {
    let outcome = worker.await;
    in_flight.finish(&request.key, request.sequence);
    let elapsed_ms = request.started.elapsed().as_millis() as u64;
    let response = match outcome {
        Ok(response) => response,
        Err(error) if error.is_cancelled() => {
            tracing::debug!(
                request_id = %request.key,
                method = %request.method,
                elapsed_ms,
                "Cancelled request stopped without a response"
            );
            return;
        }
        Err(error) => {
            tracing::error!(
                request_id = %request.key,
                method = %request.method,
                error = %error,
                "Request handler panicked"
            );
            create_error_response(
                request.id,
                -32603,
                "Internal error: the request handler failed unexpectedly",
            )
        }
    };
    tracing::debug!(
        request_id = %request.key,
        method = %request.method,
        elapsed_ms,
        "Request completed"
    );
    if let Err(error) = outbox.send(&response).await {
        tracing::warn!(request_id = %request.key, error = %error, "Response could not be sent");
    }
}

fn log_delivery_failure(joined: std::result::Result<(), JoinError>) {
    if let Err(error) = joined {
        tracing::error!(error = %error, "Response delivery task failed");
    }
}
