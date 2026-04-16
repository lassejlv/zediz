export type Role = 'owner' | 'admin' | 'member' | 'viewer';

export interface Me {
  id: string;
  email: string;
  display_name: string;
  created_at: string;
}

export interface WorkspaceSummary {
  id: string;
  slug: string;
  name: string;
  role: Role;
  created_at: string;
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
