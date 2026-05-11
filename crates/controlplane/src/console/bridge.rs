use axum::extract::ws::WebSocket;
use futures::{SinkExt, StreamExt};

/// Forward frames verbatim between a browser-side and an agent-side WebSocket
/// until either side closes. Binary frames are stdin/stdout bytes, text frames
/// carry JSON control messages (currently only `{"type":"resize",...}`). The
/// CP doesn't inspect either; the agent handles control frames at the far end.
pub async fn run(browser: WebSocket, agent: WebSocket) {
    let (mut browser_tx, mut browser_rx) = browser.split();
    let (mut agent_tx, mut agent_rx) = agent.split();

    let b2a = tokio::spawn(async move {
        while let Some(Ok(msg)) = browser_rx.next().await {
            if matches!(msg, axum::extract::ws::Message::Close(_)) {
                let _ = agent_tx.send(msg).await;
                break;
            }
            if agent_tx.send(msg).await.is_err() {
                break;
            }
        }
        let _ = agent_tx.close().await;
    });

    let a2b = tokio::spawn(async move {
        while let Some(Ok(msg)) = agent_rx.next().await {
            if matches!(msg, axum::extract::ws::Message::Close(_)) {
                let _ = browser_tx.send(msg).await;
                break;
            }
            if browser_tx.send(msg).await.is_err() {
                break;
            }
        }
        let _ = browser_tx.close().await;
    });

    let b2a_abort = b2a.abort_handle();
    let a2b_abort = a2b.abort_handle();
    tokio::select! {
        _ = b2a => { a2b_abort.abort(); },
        _ = a2b => { b2a_abort.abort(); },
    }
}
