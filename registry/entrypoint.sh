#!/bin/sh
# Driftbase registry entrypoint.
#
# Picks the storage driver based on REGISTRY_STORAGE_S3_BUCKET:
#   - set    → S3-compatible (AWS, Hetzner Object Storage, R2, MinIO, ...)
#   - unset  → local filesystem (default, identical to the previous behaviour)
#
# We render a fresh /etc/docker/registry/config.yml on every start instead of
# inheriting the image's default config. The default declares
# storage.filesystem and S3 env-var overrides are additive, so without this
# the registry would see two storage drivers and refuse to start.

set -eu

CONFIG=/etc/docker/registry/config.yml

# Common header / footer used by both backends.
write_header() {
    cat > "$CONFIG" <<'EOF'
version: 0.1
log:
  fields:
    service: registry
storage:
  cache:
    blobdescriptor: inmemory
  delete:
    enabled: true
EOF
}

write_footer() {
    cat >> "$CONFIG" <<'EOF'
http:
  addr: :5000
  headers:
    X-Content-Type-Options: [nosniff]
health:
  storagedriver:
    enabled: true
    interval: 10s
    threshold: 3
EOF
}

if [ -n "${REGISTRY_STORAGE_S3_BUCKET:-}" ]; then
    write_header
    {
        echo "  s3:"
        echo "    bucket: ${REGISTRY_STORAGE_S3_BUCKET}"
        # region is required by the s3 driver even for non-AWS providers; we
        # let the registry surface the validation error if the operator
        # forgot to set it, rather than substituting a misleading default.
        if [ -n "${REGISTRY_STORAGE_S3_REGION:-}" ]; then
            echo "    region: ${REGISTRY_STORAGE_S3_REGION}"
        fi
        if [ -n "${REGISTRY_STORAGE_S3_REGIONENDPOINT:-}" ]; then
            echo "    regionendpoint: ${REGISTRY_STORAGE_S3_REGIONENDPOINT}"
        fi
        if [ -n "${REGISTRY_STORAGE_S3_ACCESSKEY:-}" ]; then
            echo "    accesskey: ${REGISTRY_STORAGE_S3_ACCESSKEY}"
        fi
        if [ -n "${REGISTRY_STORAGE_S3_SECRETKEY:-}" ]; then
            echo "    secretkey: ${REGISTRY_STORAGE_S3_SECRETKEY}"
        fi
        echo "    forcepathstyle: ${REGISTRY_STORAGE_S3_FORCEPATHSTYLE:-true}"
        echo "    secure: ${REGISTRY_STORAGE_S3_SECURE:-true}"
        echo "    v4auth: true"
        if [ -n "${REGISTRY_STORAGE_S3_ROOTDIRECTORY:-}" ]; then
            echo "    rootdirectory: ${REGISTRY_STORAGE_S3_ROOTDIRECTORY}"
        fi
    } >> "$CONFIG"
    write_footer
    echo "[driftbase-registry] using S3 backend: bucket=${REGISTRY_STORAGE_S3_BUCKET}" >&2
else
    write_header
    cat >> "$CONFIG" <<'EOF'
  filesystem:
    rootdirectory: /var/lib/registry
EOF
    write_footer
    echo "[driftbase-registry] using filesystem backend: /var/lib/registry" >&2
fi

exec registry serve "$CONFIG"
