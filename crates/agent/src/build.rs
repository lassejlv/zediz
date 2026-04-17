use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::client::{BuildStatusBody, ControlPlaneClient, LogLineOut};

/// Parsed build command payload from the control plane.
#[derive(Debug, Deserialize)]
pub struct BuildSpec {
    pub build_id: String,
    pub deployment_id: String,
    #[allow(dead_code)]
    pub service_id: String,
    pub git_repo: String,
    pub git_branch: String,
    pub builder: String,
    /// Only meaningful when `builder == "dockerfile"`.
    #[serde(default)]
    pub dockerfile_path: Option<String>,
    pub root_dir: String,
    pub image_tag: String,
    #[serde(default)]
    pub github_pat: Option<String>,
    #[serde(default)]
    pub registry: Option<RegistryAuth>,
}

#[derive(Debug, Deserialize)]
pub struct RegistryAuth {
    pub url: String,
    pub username: String,
    pub password: String,
}

/// BuildKit frontend image used to interpret a railpack-generated plan. This
/// is the canonical image published by the railpack maintainers; BuildKit
/// pulls it on first use.
const RAILPACK_FRONTEND: &str = "ghcr.io/railwayapp/railpack-frontend";

pub async fn run_build(
    client: &ControlPlaneClient,
    node_token: &str,
    spec: BuildSpec,
) -> Result<()> {
    let work = PathBuf::from(format!("/tmp/zediz-build-{}", spec.build_id));
    // Idempotent cleanup: previous failed attempts may have left debris.
    let _ = tokio::fs::remove_dir_all(&work).await;
    tokio::fs::create_dir_all(&work).await?;

    // Always cleanup after ourselves, on success or failure.
    let result = do_build(client, node_token, &spec, &work).await;
    let _ = tokio::fs::remove_dir_all(&work).await;

    // Best-effort docker logout (no-op if we never logged in).
    if let Some(reg) = &spec.registry {
        let _ = Command::new("docker")
            .args(["logout", &reg.url])
            .output()
            .await;
    }

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            let reason = e.to_string();
            let body = BuildStatusBody {
                status: "failed".into(),
                reason: Some(reason.clone()),
                ..Default::default()
            };
            let _ = client
                .report_build_status(node_token, &spec.build_id, &body)
                .await;
            Err(e)
        }
    }
}

async fn do_build(
    client: &ControlPlaneClient,
    node_token: &str,
    spec: &BuildSpec,
    work: &Path,
) -> Result<()> {
    // 1) Clone (status=cloning).
    client
        .report_build_status(
            node_token,
            &spec.build_id,
            &BuildStatusBody {
                status: "cloning".into(),
                ..Default::default()
            },
        )
        .await?;

    let clone_url = inject_pat(&spec.git_repo, spec.github_pat.as_deref())?;
    run_logged(
        client,
        node_token,
        &spec.deployment_id,
        Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                "--branch",
                &spec.git_branch,
                &clone_url,
                ".",
            ])
            .current_dir(work),
        "git-clone",
    )
    .await?;

    // Capture the commit sha for reporting. Fine to print the full sha; short
    // form is derivable client-side.
    let git_commit = git_rev_parse(work).await?;

    // The CWD for the build is the repo's root_dir. For a monorepo this is
    // the subdir that owns the service.
    let build_cwd = resolve_root_dir(work, &spec.root_dir)?;

    // 2) Log in to the registry (if creds supplied).
    if let Some(reg) = &spec.registry {
        let mut login = Command::new("docker");
        login
            .args(["login", &reg.url, "-u", &reg.username, "--password-stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = login.spawn().context("spawn docker login")?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(reg.password.as_bytes()).await?;
            stdin.shutdown().await?;
        }
        let output = child.wait_with_output().await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("docker login failed: {}", stderr.trim());
        }
    }

    // Idempotent buildx context setup. buildx ships with modern docker.
    let _ = Command::new("docker")
        .args(["buildx", "create", "--use", "--name", "zediz-builder"])
        .output()
        .await;

    // 3) Build + push (status=building→pushing). We split these into two
    //    reports purely for UX; buildx itself doesn't expose the push phase
    //    separately in its CLI.
    client
        .report_build_status(
            node_token,
            &spec.build_id,
            &BuildStatusBody {
                status: "building".into(),
                ..Default::default()
            },
        )
        .await?;

    let meta_path = work.join("build-meta.json");
    match spec.builder.as_str() {
        "dockerfile" => {
            build_with_dockerfile(client, node_token, spec, &build_cwd, &meta_path).await?;
        }
        "railpack" => {
            build_with_railpack(client, node_token, spec, &build_cwd, &meta_path).await?;
        }
        other => bail!("unsupported builder: {other}"),
    }

    // 4) Read digest out of the metadata file buildx wrote.
    let digest = read_digest(&meta_path)
        .await
        .context("reading build metadata")?;

    client
        .report_build_status(
            node_token,
            &spec.build_id,
            &BuildStatusBody {
                status: "succeeded".into(),
                git_commit: Some(git_commit),
                image_digest: Some(digest),
                image_tag: Some(spec.image_tag.clone()),
                ..Default::default()
            },
        )
        .await?;
    Ok(())
}

async fn build_with_dockerfile(
    client: &ControlPlaneClient,
    node_token: &str,
    spec: &BuildSpec,
    cwd: &Path,
    meta_path: &Path,
) -> Result<()> {
    let dockerfile = spec
        .dockerfile_path
        .as_deref()
        .unwrap_or("Dockerfile");
    let mut cmd = Command::new("docker");
    cmd.args([
        "buildx",
        "build",
        "--platform",
        "linux/amd64",
        "--file",
        dockerfile,
        "--tag",
        &spec.image_tag,
        "--push",
        "--metadata-file",
    ])
    .arg(meta_path)
    .arg(".")
    .current_dir(cwd);

    run_logged(client, node_token, &spec.deployment_id, &mut cmd, "buildx").await
}

async fn build_with_railpack(
    client: &ControlPlaneClient,
    node_token: &str,
    spec: &BuildSpec,
    cwd: &Path,
    meta_path: &Path,
) -> Result<()> {
    // Railpack's "custom frontend" workflow:
    //   1. `railpack prepare <dir>` writes a BuildKit-consumable plan.json
    //   2. `docker buildx build -f plan.json --build-arg BUILDKIT_SYNTAX=<frontend>`
    //      hands the plan to the railpack frontend, which turns it into
    //      layers. Push + metadata-file work the same as a plain buildx.
    let plan_path = cwd.join("railpack-plan.json");
    let mut prepare = Command::new("railpack");
    prepare
        .args(["prepare", ".", "--plan-out"])
        .arg(&plan_path)
        .current_dir(cwd);
    run_logged(client, node_token, &spec.deployment_id, &mut prepare, "railpack-prepare").await?;

    let mut cmd = Command::new("docker");
    cmd.args([
        "buildx",
        "build",
        "--platform",
        "linux/amd64",
        "--build-arg",
    ])
    .arg(format!("BUILDKIT_SYNTAX={RAILPACK_FRONTEND}"))
    .args(["--file"])
    .arg(&plan_path)
    .args(["--tag", &spec.image_tag, "--push", "--metadata-file"])
    .arg(meta_path)
    .arg(".")
    .current_dir(cwd);

    run_logged(client, node_token, &spec.deployment_id, &mut cmd, "railpack-build").await
}

/// Resolve `root_dir` relative to the clone directory, rejecting paths that
/// escape it (absolute or `..` components).
fn resolve_root_dir(clone_dir: &Path, root_dir: &str) -> Result<PathBuf> {
    let rel = Path::new(root_dir.trim());
    if rel.is_absolute() {
        bail!("root_dir must be relative, got {root_dir:?}");
    }
    let mut out = clone_dir.to_path_buf();
    for comp in rel.components() {
        match comp {
            std::path::Component::Normal(c) => out.push(c),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                bail!("root_dir may not contain '..': {root_dir:?}");
            }
            _ => bail!("unsupported root_dir component in {root_dir:?}"),
        }
    }
    Ok(out)
}

/// Rewrite `https://github.com/foo/bar.git` to `https://<pat>@github.com/foo/bar.git`
/// when a PAT is supplied. Leaves other URL shapes untouched.
fn inject_pat(url: &str, pat: Option<&str>) -> Result<String> {
    let Some(pat) = pat else {
        return Ok(url.to_string());
    };
    if let Some(rest) = url.strip_prefix("https://") {
        Ok(format!("https://{pat}@{rest}"))
    } else if url.starts_with("http://") {
        Err(anyhow!("refusing to send PAT over plain http://"))
    } else {
        // SSH URLs etc. — caller is expected to have a deploy key elsewhere.
        Ok(url.to_string())
    }
}

async fn git_rev_parse(dir: &Path) -> Result<String> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .await?;
    if !out.status.success() {
        bail!("git rev-parse failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

async fn read_digest(meta_path: &Path) -> Result<String> {
    let raw = tokio::fs::read(meta_path).await?;
    let v: serde_json::Value = serde_json::from_slice(&raw)?;
    v.get("containerimage.digest")
        .and_then(|d| d.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("build metadata missing containerimage.digest"))
}

/// Run a subprocess and stream its stdout/stderr back to the control plane as
/// `[build:<tag>] …` log lines against the deployment. Waits for the process
/// to exit and returns an error if it failed.
async fn run_logged(
    client: &ControlPlaneClient,
    node_token: &str,
    deployment_id: &str,
    cmd: &mut Command,
    tag: &str,
) -> Result<()> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().with_context(|| format!("spawn {tag}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("missing stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("missing stderr"))?;

    // Fan-in channel: both pipes push lines to a single batcher that POSTs to
    // `/agent/deployments/:id/logs` every ~500ms or every 32 lines.
    let (tx, mut rx) = mpsc::channel::<LogLineOut>(128);

    let tx_out = tx.clone();
    let prefix = format!("[build:{tag}] ");
    let prefix_for_out = prefix.clone();
    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx_out
                .send(LogLineOut {
                    stream: "stdout".into(),
                    ts: Utc::now(),
                    line: format!("{prefix_for_out}{line}"),
                })
                .await;
        }
    });
    let prefix_for_err = prefix;
    tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx
                .send(LogLineOut {
                    stream: "stderr".into(),
                    ts: Utc::now(),
                    line: format!("{prefix_for_err}{line}"),
                })
                .await;
        }
    });

    let client = client.clone();
    let node_token = node_token.to_string();
    let deployment_id = deployment_id.to_string();
    let pusher = tokio::spawn(async move {
        let mut buf: Vec<LogLineOut> = Vec::with_capacity(32);
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(500));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                maybe = rx.recv() => {
                    match maybe {
                        Some(line) => {
                            buf.push(line);
                            if buf.len() >= 32 {
                                let batch = std::mem::take(&mut buf);
                                let _ = client.push_logs(&node_token, &deployment_id, batch).await;
                            }
                        }
                        None => {
                            // channel closed — flush and exit
                            if !buf.is_empty() {
                                let _ = client.push_logs(&node_token, &deployment_id, buf).await;
                            }
                            break;
                        }
                    }
                }
                _ = ticker.tick() => {
                    if !buf.is_empty() {
                        let batch = std::mem::take(&mut buf);
                        let _ = client.push_logs(&node_token, &deployment_id, batch).await;
                    }
                }
            }
        }
    });

    let status = child.wait().await?;
    // Give the pusher a moment to drain — its receiver closes when both stream
    // tasks drop their senders, which happens once the child's pipes EOF.
    let _ = pusher.await;

    if !status.success() {
        bail!("{tag} exited with {status}");
    }
    Ok(())
}
