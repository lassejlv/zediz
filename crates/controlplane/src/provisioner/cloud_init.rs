/// Render the cloud-init `user_data` that installs Docker, pulls the prebuilt
/// `driftbase-agent` image, and runs it under systemd. Boot-to-ready is ~60–90s
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
  - iproute2
  - iptables
  - wireguard-tools
write_files:
  - path: /etc/driftbase/agent.env
    owner: root:root
    permissions: '0600'
    content: |
      DRIFTBASE_CONTROL_PLANE_URL={control_plane_url}
      DRIFTBASE_BOOTSTRAP_TOKEN={bootstrap_token}
      DRIFTBASE_NODE_ID={node_id}
      DRIFTBASE_WORKSPACE_ID={workspace_id}
      DRIFTBASE_AGENT_IMAGE={agent_image}
      DRIFTBASE_PRIVATE_NETWORK_DIR=/var/lib/driftbase/network
  - path: /etc/systemd/system/driftbase-agent.service
    owner: root:root
    permissions: '0644'
    content: |
      [Unit]
      Description=Driftbase node agent
      After=docker.service network-online.target
      Wants=network-online.target
      Requires=docker.service

      [Service]
      Type=simple
      EnvironmentFile=/etc/driftbase/agent.env
      ExecStartPre=-/usr/bin/docker rm -f driftbase-agent
      ExecStartPre=/usr/bin/docker pull $DRIFTBASE_AGENT_IMAGE
      ExecStartPre=/usr/bin/mkdir -p /var/lib/driftbase/volumes
      ExecStartPre=/usr/bin/mkdir -p /var/lib/driftbase/network
      # rshared propagation requires the host parent to itself be a
      # shared mount. Dirs under /var are private by default, so bind
      # the volumes dir over itself and flip it shared before the agent
      # container starts. Without this, mounts the agent makes inside
      # its namespace don't propagate to the host, so Docker can't see
      # the volume when binding it into a service container.
      #
      # systemd ExecStartPre runs commands directly (no shell), so we
      # wrap in /bin/sh -c to use the || guard for idempotence.
      ExecStartPre=/bin/sh -c 'mountpoint -q /var/lib/driftbase/volumes || mount --bind /var/lib/driftbase/volumes /var/lib/driftbase/volumes'
      ExecStartPre=/bin/sh -c 'mount --make-rshared /var/lib/driftbase/volumes'
      ExecStart=/usr/bin/docker run --rm --name driftbase-agent \
        --network host \
        --env-file /etc/driftbase/agent.env \
        -v /var/run/docker.sock:/var/run/docker.sock \
        -v /dev:/dev \
        -v /etc/driftbase:/etc/driftbase \
        -v /var/lib/driftbase/volumes:/var/lib/driftbase/volumes:rshared \
        -v /var/lib/driftbase/network:/var/lib/driftbase/network \
        --cap-add=SYS_ADMIN \
        --cap-add=NET_ADMIN \
        --security-opt apparmor=unconfined \
        $DRIFTBASE_AGENT_IMAGE
      ExecStop=/usr/bin/docker stop driftbase-agent
      Restart=always
      RestartSec=5s

      [Install]
      WantedBy=multi-user.target
runcmd:
  - curl -fsSL https://get.docker.com | sh
  - systemctl daemon-reload
  - systemctl enable --now driftbase-agent.service
"#
    )
}
