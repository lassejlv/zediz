-- Custom (BYO) domains attached to services.
-- Each domain maps a public hostname to a (service, container_port). Caddy on
-- the node hosting the deployment issues a Let's Encrypt cert for it.
CREATE TABLE service_domains (
    id              TEXT PRIMARY KEY,
    service_id      TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    hostname        TEXT NOT NULL,
    container_port  INTEGER NOT NULL,
    tls_status      TEXT NOT NULL DEFAULT 'pending' CHECK (tls_status IN (
        'pending', 'active', 'failed'
    )),
    last_error      TEXT,
    last_cert_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (hostname)
);
CREATE INDEX service_domains_service_idx ON service_domains(service_id);
