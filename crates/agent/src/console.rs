use anyhow::{anyhow, bail, Context, Result};
use bollard::exec::{CreateExecOptions, ResizeExecOptions, StartExecOptions, StartExecResults};
use bollard::Docker;
use futures::{SinkExt, StreamExt};
use http::header::AUTHORIZATION;
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;

/// Open a single console session: dial the control plane WebSocket using the
/// `session_id`, start a Docker exec with a PTY inside the target container,
/// and bridge stdin / stdout / resize control frames between the two until
/// either side closes. Runs in a detached tokio task spawned by the executor.
pub async fn run_session(
    cp_base: String,
    node_token: String,
    session_id: String,
    container_name: String,
    cols: u16,
    rows: u16,
    docker: Docker,
) -> Result<()> {
    let ws_url = build_ws_url(&cp_base, &session_id)?;
    let mut request = ws_url
        .as_str()
        .into_client_request()
        .with_context(|| format!("building console ws request for {ws_url}"))?;
    request.headers_mut().insert(
        AUTHORIZATION,
        format!("Bearer {node_token}")
            .parse()
            .map_err(|e| anyhow!("invalid bearer header: {e}"))?,
    );

    let (ws_stream, _resp) = tokio_tungstenite::connect_async(request)
        .await
        .with_context(|| format!("dialing console ws {ws_url}"))?;

    let exec = docker
        .create_exec(
            &container_name,
            CreateExecOptions {
                attach_stdin: Some(true),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                tty: Some(true),
                cmd: Some(vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "command -v bash >/dev/null && exec bash || exec sh".to_string(),
                ]),
                env: Some(vec!["TERM=xterm-256color".to_string()]),
                ..Default::default()
            },
        )
        .await
        .with_context(|| format!("create_exec on {container_name}"))?;

    let started = docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                tty: true,
                output_capacity: None,
            }),
        )
        .await
        .context("start_exec")?;

    let (mut output, mut input) = match started {
        StartExecResults::Attached { output, input } => (output, input),
        StartExecResults::Detached => bail!("docker returned detached exec, expected attached"),
    };

    let _ = docker
        .resize_exec(
            &exec.id,
            ResizeExecOptions {
                width: cols,
                height: rows,
            },
        )
        .await;

    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let exec_id_for_resize = exec.id.clone();
    let docker_for_resize = docker.clone();

    // stdin task: WebSocket -> docker stdin (and handle resize control frames).
    let stdin = async move {
        while let Some(msg) = ws_rx.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    tracing::debug!(error = ?e, "console ws recv error");
                    break;
                }
            };
            match msg {
                Message::Binary(bytes) => {
                    if input.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
                Message::Text(text) => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        if v.get("type").and_then(|t| t.as_str()) == Some("resize") {
                            let cols = v
                                .get("cols")
                                .and_then(|c| c.as_u64())
                                .unwrap_or(80)
                                .clamp(2, 1000) as u16;
                            let rows = v
                                .get("rows")
                                .and_then(|r| r.as_u64())
                                .unwrap_or(24)
                                .clamp(2, 1000) as u16;
                            let _ = docker_for_resize
                                .resize_exec(
                                    &exec_id_for_resize,
                                    ResizeExecOptions {
                                        width: cols,
                                        height: rows,
                                    },
                                )
                                .await;
                        }
                    }
                }
                Message::Ping(_) | Message::Pong(_) => {}
                Message::Close(_) => break,
                Message::Frame(_) => {}
            }
        }
        let _ = input.shutdown().await;
    };

    // stdout task: docker stdout -> WebSocket (TTY mode emits Console frames).
    let stdout = async move {
        while let Some(frame) = output.next().await {
            let bytes = match frame {
                Ok(out) => out.into_bytes(),
                Err(e) => {
                    tracing::debug!(error = ?e, "docker exec stream error");
                    break;
                }
            };
            if ws_tx.send(Message::Binary(bytes.to_vec())).await.is_err() {
                break;
            }
        }
        let _ = ws_tx.close().await;
    };

    tokio::select! {
        _ = stdin => {},
        _ = stdout => {},
    }
    Ok(())
}

fn build_ws_url(cp_base: &str, session_id: &str) -> Result<String> {
    let scheme_swapped = if let Some(rest) = cp_base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = cp_base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        bail!("control plane URL must start with http(s)://: {cp_base}");
    };
    Ok(format!(
        "{}/api/v1/agent/console/{}/ws",
        scheme_swapped.trim_end_matches('/'),
        session_id
    ))
}
