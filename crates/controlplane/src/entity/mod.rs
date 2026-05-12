#![allow(dead_code)]

use sea_orm::entity::prelude::*;

pub mod users {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "users")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub email: String,
        pub password_hash: String,
        pub display_name: String,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
        pub status: String,
        pub is_platform_admin: bool,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod sessions {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sessions")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub user_id: String,
        pub token_hash: Vec<u8>,
        pub user_agent: Option<String>,
        pub ip: Option<String>,
        pub created_at: DateTimeUtc,
        pub expires_at: DateTimeUtc,
        pub revoked_at: Option<DateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod workspaces {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "workspaces")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub slug: String,
        pub name: String,
        pub owner_user_id: String,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
        pub hetzner_location: String,
        pub default_server_type: Option<String>,
        pub max_nodes: i32,
        pub max_monthly_euro: i32,
        pub autoscale_idle_ttl_seconds: i32,
        pub scheduler_paused_until: Option<DateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod workspace_members {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "workspace_members")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub workspace_id: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub user_id: String,
        pub role: String,
        pub joined_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod invites {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "invites")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub workspace_id: String,
        pub email: String,
        pub role: String,
        pub token_hash: Vec<u8>,
        pub invited_by: String,
        pub created_at: DateTimeUtc,
        pub expires_at: DateTimeUtc,
        pub accepted_by: Option<String>,
        pub accepted_at: Option<DateTimeUtc>,
        pub revoked_at: Option<DateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod credentials {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "credentials")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub workspace_id: String,
        pub kind: String,
        pub name: String,
        pub encrypted: Vec<u8>,
        pub metadata: Json,
        pub created_by: String,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
        pub hetzner_location: String,
        pub last_used_at: Option<DateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod ssh_keys {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "ssh_keys")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub workspace_id: String,
        pub name: String,
        pub public_key: String,
        pub fingerprint: String,
        pub private_key_encrypted: Option<Vec<u8>>,
        pub hetzner_key_id: Option<i64>,
        pub created_by: String,
        pub created_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod projects {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "projects")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub workspace_id: String,
        pub slug: String,
        pub name: String,
        pub created_by: String,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod services {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "services")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub project_id: String,
        pub slug: String,
        pub name: String,
        pub source: String,
        pub image_ref: Option<String>,
        pub env_vars: Json,
        pub ports: Json,
        pub resources: Json,
        pub replicas: i32,
        pub restart_policy: String,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
        pub git_repo: Option<String>,
        pub git_branch: Option<String>,
        pub git_commit: Option<String>,
        pub dockerfile_path: Option<String>,
        pub root_dir: Option<String>,
        pub registry_repo: Option<String>,
        pub github_credential_id: Option<String>,
        pub registry_credential_id: Option<String>,
        pub builder: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod nodes {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "nodes")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub workspace_id: String,
        pub name: String,
        pub provider: String,
        pub provider_node_id: Option<String>,
        pub status: String,
        pub total_cpu_millis: i32,
        pub total_memory_mb: i32,
        pub total_disk_mb: i32,
        pub labels: Json,
        pub agent_version: Option<String>,
        pub last_seen_at: Option<DateTimeUtc>,
        pub created_at: DateTimeUtc,
        pub bootstrap_token_hash: Option<String>,
        pub node_token_hash: Option<String>,
        pub hetzner_server_id: Option<i64>,
        pub hetzner_location: Option<String>,
        pub hetzner_server_type: Option<String>,
        pub public_ipv4: Option<String>,
        pub persistent: bool,
        pub idle_since_at: Option<DateTimeUtc>,
        pub registered_at: Option<DateTimeUtc>,
        pub private_network_capable: bool,
        pub wireguard_public_key: Option<String>,
        pub wireguard_mesh_ip: Option<String>,
        pub wireguard_listen_port: i32,
        pub private_network_synced_at: Option<DateTimeUtc>,
        pub private_network_sync_error: Option<String>,
        pub agent_image_ref: Option<String>,
        pub agent_image_digest: Option<String>,
        pub agent_self_update_capable: bool,
        pub agent_update_status: String,
        pub agent_update_checked_at: Option<DateTimeUtc>,
        pub agent_update_target_image_ref: Option<String>,
        pub agent_update_target_digest: Option<String>,
        pub agent_update_command_id: Option<String>,
        pub agent_update_error: Option<String>,
        pub agent_update_started_at: Option<DateTimeUtc>,
        pub agent_update_finished_at: Option<DateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod deployments {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "deployments")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub service_id: String,
        pub node_id: Option<String>,
        pub status: String,
        pub image_ref: String,
        pub env_vars: Json,
        pub ports: Json,
        pub resources: Json,
        pub container_id: Option<String>,
        pub reason: Option<String>,
        pub created_at: DateTimeUtc,
        pub started_at: Option<DateTimeUtc>,
        pub stopped_at: Option<DateTimeUtc>,
        pub updated_at: DateTimeUtc,
        pub runtime_metrics: Option<Json>,
        pub private_ipv4: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod node_allocations {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "node_allocations")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub node_id: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub deployment_id: String,
        pub cpu_millis: i32,
        pub memory_mb: i32,
        pub disk_mb: i32,
        pub created_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod agent_commands {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "agent_commands")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub node_id: String,
        pub deployment_id: Option<String>,
        pub kind: String,
        pub payload: Json,
        pub status: String,
        pub result: Option<String>,
        pub created_at: DateTimeUtc,
        pub dispatched_at: Option<DateTimeUtc>,
        pub acked_at: Option<DateTimeUtc>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod deployment_logs {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "deployment_logs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub deployment_id: String,
        pub stream: String,
        pub ts: DateTimeUtc,
        pub line: String,
        pub received_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod service_domains {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "service_domains")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub service_id: String,
        pub hostname: String,
        pub container_port: i32,
        pub tls_status: String,
        pub last_error: Option<String>,
        pub last_cert_at: Option<DateTimeUtc>,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod builds {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "builds")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub service_id: String,
        pub deployment_id: Option<String>,
        pub node_id: Option<String>,
        pub status: String,
        pub git_commit: Option<String>,
        pub image_digest: Option<String>,
        pub image_tag: Option<String>,
        pub reason: Option<String>,
        pub created_at: DateTimeUtc,
        pub started_at: Option<DateTimeUtc>,
        pub finished_at: Option<DateTimeUtc>,
        pub updated_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod deployment_metrics {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "deployment_metrics")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub deployment_id: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub ts: DateTimeUtc,
        pub cpu_percent: f32,
        pub memory_bytes: i64,
        pub memory_limit_bytes: Option<i64>,
        pub rx_bytes: i64,
        pub tx_bytes: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod volumes {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "volumes")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub workspace_id: String,
        pub name: String,
        pub size_gb: i32,
        pub hetzner_volume_id: Option<i64>,
        pub hetzner_location: String,
        pub attached_node_id: Option<String>,
        pub attached_service_id: Option<String>,
        pub mount_path: Option<String>,
        pub status: String,
        pub reason: Option<String>,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod project_networks {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "project_networks")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub project_id: String,
        pub cidr: String,
        pub domain: String,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod project_network_node_subnets {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "project_network_node_subnets")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub project_network_id: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub node_id: String,
        pub cidr: String,
        pub gateway_ip: String,
        pub dns_ip: String,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
