export type Role = 'owner' | 'admin' | 'member' | 'viewer';

export interface Me {
  id: string;
  email: string;
  display_name: string;
  created_at: string;
  is_platform_admin: boolean;
}

export interface WorkspaceSummary {
  id: string;
  slug: string;
  name: string;
  role: Role;
  created_at: string;
  hetzner_location?: string | null;
  default_server_type?: string | null;
  max_nodes?: number | null;
  max_monthly_euro?: number | null;
  autoscale_idle_ttl_seconds?: number | null;
}

export interface MemberRow {
  user_id: string;
  email: string;
  display_name: string;
  role: Role;
  joined_at: string;
}

export interface InviteSummary {
  id: string;
  email: string;
  role: Role;
  expires_at: string;
  created_at: string;
  accepted_at: string | null;
}

export interface CreatedInvite extends InviteSummary {
  token: string;
  accept_url: string;
}

export type CredentialKind = 'hetzner_api_token' | 'github_pat' | 'registry';

export interface CredentialSummary {
  id: string;
  kind: CredentialKind;
  name: string;
  metadata: Record<string, unknown>;
  created_at: string;
  updated_at: string;
  last_used_at: string | null;
}

export interface SshKeySummary {
  id: string;
  name: string;
  public_key: string;
  fingerprint: string;
  has_private_key: boolean;
  hetzner_key_id: number | null;
  created_at: string;
}

export interface ProjectSummary {
  id: string;
  slug: string;
  name: string;
  private_network_enabled: boolean;
  private_network_domain: string;
  created_at: string;
}

export interface Resources {
  cpu_millis: number;
  memory_mb: number;
  disk_mb: number;
}

export interface PortMap {
  container_port: number;
  host_port: number | null;
  protocol: string;
}

export type EnvVars = Record<string, string>;

export type VariableReferenceKind = 'env' | 'generated';

export interface VariableReference {
  key: string;
  kind: VariableReferenceKind;
  expression: string;
}

export interface VariableReferenceService {
  slug: string;
  name: string;
  variables: VariableReference[];
}

export interface VariableReferencesResponse {
  services: VariableReferenceService[];
}

export type RestartPolicy = 'no' | 'on-failure' | 'always';

export type ServiceSource = 'image' | 'git';
export type ServiceBuilder = 'dockerfile' | 'railpack';

export interface ServiceSummary {
  id: string;
  slug: string;
  name: string;
  private_hostname: string;
  source: ServiceSource;
  image_ref: string | null;
  env_vars: EnvVars;
  ports: PortMap[];
  resources: Resources;
  replicas: number;
  restart_policy: RestartPolicy;
  git_repo: string | null;
  git_branch: string | null;
  git_commit: string | null;
  dockerfile_path: string | null;
  root_dir: string | null;
  builder: ServiceBuilder;
  registry_repo: string | null;
  github_credential_id: string | null;
  registry_credential_id: string | null;
  created_at: string;
  updated_at: string;
}

export type DeploymentStatus =
  | 'pending'
  | 'building'
  | 'placing'
  | 'pulling'
  | 'starting'
  | 'running'
  | 'failing'
  | 'stopped'
  | 'errored';

export type BuildStatus =
  | 'queued'
  | 'cloning'
  | 'building'
  | 'pushing'
  | 'succeeded'
  | 'failed'
  | 'cancelled';

export interface BuildSummary {
  id: string;
  service_id: string;
  deployment_id: string | null;
  node_id: string | null;
  status: BuildStatus;
  git_commit: string | null;
  image_digest: string | null;
  image_tag: string | null;
  reason: string | null;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  updated_at: string;
}

export interface RuntimeMetrics {
  deployment_id: string;
  ts: string;
  cpu_percent: number;
  memory_bytes: number;
  memory_limit_bytes?: number | null;
  rx_bytes: number;
  tx_bytes: number;
}

export interface DeploymentSummary {
  id: string;
  service_id: string;
  node_id: string | null;
  status: DeploymentStatus;
  image_ref: string;
  container_id: string | null;
  reason: string | null;
  created_at: string;
  started_at: string | null;
  stopped_at: string | null;
  updated_at: string;
  runtime_metrics?: RuntimeMetrics | null;
}

export type TlsStatus = 'pending' | 'active' | 'failed';

export interface DomainSummary {
  id: string;
  service_id: string;
  hostname: string;
  container_port: number;
  tls_status: TlsStatus;
  last_error: string | null;
  last_cert_at: string | null;
  created_at: string;
}

export interface NodeSummary {
  id: string;
  name: string;
  provider: string;
  status: string;
  total_cpu_millis: number;
  total_memory_mb: number;
  total_disk_mb: number;
  used_cpu_millis: number;
  used_memory_mb: number;
  used_disk_mb: number;
  labels: Record<string, unknown>;
  public_ipv4: string | null;
  agent_version: string | null;
  agent_image_ref: string | null;
  agent_image_digest: string | null;
  agent_self_update_capable: boolean;
  agent_update_status: string;
  agent_update_checked_at: string | null;
  agent_update_target_image_ref: string | null;
  agent_update_target_digest: string | null;
  agent_update_command_id: string | null;
  agent_update_error: string | null;
  agent_update_started_at: string | null;
  agent_update_finished_at: string | null;
  private_network_capable: boolean;
  wireguard_mesh_ip: string | null;
  private_network_synced_at: string | null;
  private_network_sync_error: string | null;
  last_seen_at: string | null;
  created_at: string;
  workloads: NodeWorkloadSummary[];
}

export interface NodeWorkloadSummary {
  kind: 'build' | 'runtime';
  status: string;
  project_slug: string;
  service_slug: string;
  deployment_id: string;
  build_id: string | null;
  cpu_millis: number;
  memory_mb: number;
  disk_mb: number;
}
