-- Extensions
CREATE EXTENSION IF NOT EXISTS citext;

-- Users
CREATE TABLE users (
    id              TEXT PRIMARY KEY,
    email           CITEXT NOT NULL UNIQUE,
    password_hash   TEXT NOT NULL,
    display_name    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Sessions
CREATE TABLE sessions (
    id              TEXT PRIMARY KEY,
    user_id         TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash      BYTEA NOT NULL UNIQUE,
    user_agent      TEXT,
    ip              TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL,
    revoked_at      TIMESTAMPTZ
);
CREATE INDEX sessions_user_id_idx ON sessions(user_id);
CREATE INDEX sessions_expires_at_idx ON sessions(expires_at);

-- Workspaces
CREATE TABLE workspaces (
    id              TEXT PRIMARY KEY,
    slug            TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    owner_user_id   TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX workspaces_owner_idx ON workspaces(owner_user_id);

-- Workspace membership with roles
CREATE TABLE workspace_members (
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id         TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
    joined_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, user_id)
);
CREATE INDEX workspace_members_user_idx ON workspace_members(user_id);

-- Invites
CREATE TABLE invites (
    id              TEXT PRIMARY KEY,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    email           CITEXT NOT NULL,
    role            TEXT NOT NULL CHECK (role IN ('admin', 'member', 'viewer')),
    token_hash      BYTEA NOT NULL UNIQUE,
    invited_by      TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL,
    accepted_by     TEXT REFERENCES users(id) ON DELETE SET NULL,
    accepted_at     TIMESTAMPTZ,
    revoked_at      TIMESTAMPTZ
);
CREATE INDEX invites_workspace_idx ON invites(workspace_id);
CREATE UNIQUE INDEX invites_pending_unique
    ON invites(workspace_id, email)
    WHERE accepted_at IS NULL AND revoked_at IS NULL;
