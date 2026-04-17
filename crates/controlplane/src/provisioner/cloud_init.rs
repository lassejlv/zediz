/// Render the cloud-init `user_data` that installs Docker, pulls the prebuilt
/// `zediz-agent` image, and runs it under systemd. Boot-to-ready is ~60–90s
/// because all Rust compilation happens upstream in CI.
pub fn render(
    control_plane_url: &str,
    bootstrap_token: &str,
    agent_image: &str,
    node_id: &str,
    workspace_id: &str,
) -> String {
    format!(
        r#"#cloud-config
package_update: true
packages:
  - ca-certificates
  - curl
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
      After=docker.service network-online.target
      Wants=network-online.target
      Requires=docker.service

      [Service]
      Type=simple
      ExecStartPre=-/usr/bin/docker rm -f zediz-agent
      ExecStartPre=/usr/bin/docker pull {agent_image}
      ExecStartPre=/usr/bin/mkdir -p /var/lib/zediz/volumes
      ExecStart=/usr/bin/docker run --rm --name zediz-agent \
        --network host \
        --env-file /etc/zediz/agent.env \
        -v /var/run/docker.sock:/var/run/docker.sock \
        -v /dev:/dev \
        -v /var/lib/zediz/volumes:/var/lib/zediz/volumes:rshared \
        --cap-add=SYS_ADMIN \
        --security-opt apparmor=unconfined \
        {agent_image}
      ExecStop=/usr/bin/docker stop zediz-agent
      Restart=always
      RestartSec=5s

      [Install]
      WantedBy=multi-user.target
runcmd:
  - curl -fsSL https://get.docker.com | sh
  - systemctl daemon-reload
  - systemctl enable --now zediz-agent.service
"#
    )
}
