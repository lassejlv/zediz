/// Render the cloud-init `user_data` that installs Docker + Rust, clones the
/// Zediz repo, builds `zediz-agent` from source, and starts it as a systemd unit.
///
/// Building on the VM avoids a release pipeline at the cost of ~5–15 minutes
/// extra boot time. `git_repo` defaults to the public Zediz repo; override with
/// `ZEDIZ_AGENT_GIT_REPO`, and `ZEDIZ_AGENT_GIT_REF` (branch/tag/sha, default `main`).
pub fn render(
    control_plane_url: &str,
    bootstrap_token: &str,
    git_repo: &str,
    git_ref: &str,
    node_id: &str,
    workspace_id: &str,
) -> String {
    format!(
        r#"#cloud-config
package_update: true
packages:
  - ca-certificates
  - curl
  - git
  - build-essential
  - pkg-config
  - libssl-dev
write_files:
  - path: /etc/zediz/agent.env
    owner: root:root
    permissions: '0600'
    content: |
      ZEDIZ_CONTROL_PLANE_URL={control_plane_url}
      ZEDIZ_BOOTSTRAP_TOKEN={bootstrap_token}
      ZEDIZ_NODE_ID={node_id}
      ZEDIZ_WORKSPACE_ID={workspace_id}
  - path: /etc/systemd/system/zediz-agent.service
    owner: root:root
    permissions: '0644'
    content: |
      [Unit]
      Description=Zediz node agent
      After=network-online.target docker.service
      Wants=network-online.target docker.service
      Requires=docker.service

      [Service]
      Type=simple
      EnvironmentFile=/etc/zediz/agent.env
      ExecStart=/usr/local/bin/zediz-agent
      Restart=always
      RestartSec=5s

      [Install]
      WantedBy=multi-user.target
  - path: /usr/local/sbin/zediz-install-agent.sh
    owner: root:root
    permissions: '0755'
    content: |
      #!/usr/bin/env bash
      set -euxo pipefail

      # Docker
      if ! command -v docker >/dev/null 2>&1; then
        curl -fsSL https://get.docker.com | sh
      fi

      # Rust (rustup), if not already installed
      if ! command -v cargo >/dev/null 2>&1; then
        curl -fsSL https://sh.rustup.rs -o /tmp/rustup-init.sh
        sh /tmp/rustup-init.sh -y --default-toolchain stable --profile minimal
        source $HOME/.cargo/env
      fi
      export PATH="$HOME/.cargo/bin:$PATH"

      # Build agent from source
      mkdir -p /opt/zediz
      cd /opt/zediz
      if [ ! -d .git ]; then
        git clone --depth 1 --branch {git_ref} {git_repo} .
      else
        git fetch --depth 1 origin {git_ref} && git reset --hard FETCH_HEAD
      fi
      cargo build --release -p zediz-agent
      install -m 0755 target/release/zediz-agent /usr/local/bin/zediz-agent

      # Start the service
      systemctl daemon-reload
      systemctl enable --now zediz-agent.service
runcmd:
  - /usr/local/sbin/zediz-install-agent.sh >> /var/log/zediz-install.log 2>&1
"#
    )
}
