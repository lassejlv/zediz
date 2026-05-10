use std::collections::BTreeMap;

use sea_orm::DatabaseConnection;
use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::error::{ApiError, ApiResult};
use crate::services::EnvVars;

const MAX_REFERENCE_DEPTH: usize = 16;
pub const PRIVATE_HOSTNAME_KEY: &str = "DRIFTBASE_PRIVATE_HOSTNAME";

#[derive(Debug, Serialize)]
pub struct VariableReferencesResponse {
    pub services: Vec<VariableReferenceService>,
}

#[derive(Debug, Serialize)]
pub struct VariableReferenceService {
    pub slug: String,
    pub name: String,
    pub variables: Vec<VariableReference>,
}

#[derive(Debug, Serialize)]
pub struct VariableReference {
    pub key: String,
    pub kind: &'static str,
    pub expression: String,
}

#[derive(Debug, Clone)]
struct ServiceVars {
    slug: String,
    name: String,
    env: EnvVars,
}

#[derive(sea_orm::FromQueryResult)]
struct ServiceEnvRow {
    id: String,
    slug: String,
    name: String,
    env_vars: JsonValue,
}

pub async fn resolve_for_deployment(
    pool: &DatabaseConnection,
    project_id: &str,
    current_service_id: &str,
    current_slug: &str,
    current_env: &EnvVars,
) -> ApiResult<EnvVars> {
    let services =
        load_project_services(pool, project_id, Some((current_service_id, current_env))).await?;
    resolve_current_service_env(current_slug, services).map_err(ApiError::Validation)
}

pub async fn variable_references(
    pool: &DatabaseConnection,
    project_id: &str,
) -> ApiResult<VariableReferencesResponse> {
    let services = load_project_services(pool, project_id, None).await?;
    Ok(VariableReferencesResponse {
        services: services.into_iter().map(reference_service).collect(),
    })
}

async fn load_project_services(
    pool: &DatabaseConnection,
    project_id: &str,
    current_override: Option<(&str, &EnvVars)>,
) -> ApiResult<Vec<ServiceVars>> {
    let rows: Vec<ServiceEnvRow> = crate::db::query_as(
        "SELECT id, slug, name, env_vars FROM services WHERE project_id = $1 ORDER BY slug ASC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let env = match current_override {
                Some((current_id, env)) if current_id == row.id => env.clone(),
                _ => serde_json::from_value(row.env_vars)
                    .map_err(|e| ApiError::Internal(anyhow::anyhow!("env_vars: {e}")))?,
            };
            Ok(ServiceVars {
                slug: row.slug,
                name: row.name,
                env,
            })
        })
        .collect()
}

fn reference_service(service: ServiceVars) -> VariableReferenceService {
    let mut variables = Vec::new();
    for key in service.env.keys().filter(|key| is_valid_env_key(key)) {
        variables.push(VariableReference {
            key: key.clone(),
            kind: "env",
            expression: scoped_expression(&service.slug, key),
        });
    }
    if !service.env.contains_key(PRIVATE_HOSTNAME_KEY) {
        variables.push(VariableReference {
            key: PRIVATE_HOSTNAME_KEY.to_string(),
            kind: "generated",
            expression: scoped_expression(&service.slug, PRIVATE_HOSTNAME_KEY),
        });
    }

    variables.sort_by(|a, b| a.key.cmp(&b.key).then_with(|| a.kind.cmp(b.kind)));

    VariableReferenceService {
        slug: service.slug,
        name: service.name,
        variables,
    }
}

fn resolve_current_service_env(
    current_slug: &str,
    services: Vec<ServiceVars>,
) -> Result<EnvVars, String> {
    let services_by_slug = services
        .into_iter()
        .map(|service| (service.slug.clone(), service))
        .collect::<BTreeMap<_, _>>();

    let current = services_by_slug
        .get(current_slug)
        .ok_or_else(|| format!("unknown service reference '{current_slug}'"))?;

    let mut memo = BTreeMap::new();
    let mut rendered = EnvVars::new();
    for key in current.env.keys() {
        let value = resolve_key(
            current_slug,
            key,
            &services_by_slug,
            &mut memo,
            &mut Vec::new(),
            0,
        )?;
        rendered.insert(key.clone(), value);
    }
    Ok(rendered)
}

fn render_value(
    raw: &str,
    owner_slug: &str,
    services_by_slug: &BTreeMap<String, ServiceVars>,
    memo: &mut BTreeMap<(String, String), String>,
    stack: &mut Vec<(String, String)>,
    depth: usize,
) -> Result<String, String> {
    if depth > MAX_REFERENCE_DEPTH {
        return Err("variable reference depth exceeded".to_string());
    }

    let mut rendered = String::with_capacity(raw.len());
    let mut cursor = 0;
    while let Some(relative_start) = raw[cursor..].find("${{") {
        let start = cursor + relative_start;
        rendered.push_str(&raw[cursor..start]);

        let inner_start = start + 3;
        let Some(relative_end) = raw[inner_start..].find("}}") else {
            return Err("malformed variable reference: missing closing '}}'".to_string());
        };
        let end = inner_start + relative_end;
        let token = &raw[inner_start..end];
        let target = parse_reference(token)?;
        let target_slug = target.service_slug.unwrap_or(owner_slug);
        let replacement = resolve_key(
            target_slug,
            &target.key,
            services_by_slug,
            memo,
            stack,
            depth + 1,
        )?;
        rendered.push_str(&replacement);
        cursor = end + 2;
    }
    rendered.push_str(&raw[cursor..]);
    Ok(rendered)
}

fn resolve_key(
    service_slug: &str,
    key: &str,
    services_by_slug: &BTreeMap<String, ServiceVars>,
    memo: &mut BTreeMap<(String, String), String>,
    stack: &mut Vec<(String, String)>,
    depth: usize,
) -> Result<String, String> {
    if depth > MAX_REFERENCE_DEPTH {
        return Err("variable reference depth exceeded".to_string());
    }
    if !is_valid_env_key(key) {
        return Err(format!("invalid variable reference key '{key}'"));
    }

    let service = services_by_slug
        .get(service_slug)
        .ok_or_else(|| format!("unknown service reference '{service_slug}'"))?;
    let stack_key = (service_slug.to_string(), key.to_string());

    if let Some(value) = memo.get(&stack_key) {
        return Ok(value.clone());
    }
    if stack.iter().any(|entry| entry == &stack_key) {
        let mut cycle = stack
            .iter()
            .map(|(slug, key)| format!("{slug}.{key}"))
            .collect::<Vec<_>>();
        cycle.push(format!("{service_slug}.{key}"));
        return Err(format!(
            "cyclic variable reference detected: {}",
            cycle.join(" -> ")
        ));
    }

    let Some(raw_value) = service
        .env
        .get(key)
        .cloned()
        .or_else(|| generated_value(service_slug, key))
    else {
        return Err(format!("unknown variable reference '{service_slug}.{key}'"));
    };

    stack.push(stack_key.clone());
    let rendered = render_value(
        &raw_value,
        service_slug,
        services_by_slug,
        memo,
        stack,
        depth + 1,
    )?;
    stack.pop();

    memo.insert(stack_key, rendered.clone());
    Ok(rendered)
}

struct ReferenceTarget<'a> {
    service_slug: Option<&'a str>,
    key: String,
}

fn parse_reference(token: &str) -> Result<ReferenceTarget<'_>, String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("malformed variable reference: empty token".to_string());
    }

    let mut parts = token.split('.');
    let first = parts.next().unwrap_or_default().trim();
    let second = parts.next().map(str::trim);
    if parts.next().is_some() {
        return Err(format!("malformed variable reference '{token}'"));
    }

    match second {
        Some(key) => {
            if first.is_empty() || key.is_empty() {
                return Err(format!("malformed variable reference '{token}'"));
            }
            if !is_valid_service_slug(first) {
                return Err(format!("invalid service reference '{first}'"));
            }
            if !is_valid_env_key(key) {
                return Err(format!("invalid variable reference key '{key}'"));
            }
            Ok(ReferenceTarget {
                service_slug: Some(first),
                key: key.to_string(),
            })
        }
        None => {
            if !is_valid_env_key(first) {
                return Err(format!("invalid variable reference key '{first}'"));
            }
            Ok(ReferenceTarget {
                service_slug: None,
                key: first.to_string(),
            })
        }
    }
}

fn generated_value(service_slug: &str, key: &str) -> Option<String> {
    (key == PRIVATE_HOSTNAME_KEY).then(|| crate::private_network::private_hostname(service_slug))
}

fn scoped_expression(service_slug: &str, key: &str) -> String {
    format!("${{{{{}.{}}}}}", service_slug, key)
}

fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn is_valid_service_slug(slug: &str) -> bool {
    let mut chars = slug.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    let mut last = first;
    for c in chars {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-' {
            return false;
        }
        last = c;
    }
    last.is_ascii_lowercase() || last.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(slug: &str, env: &[(&str, &str)]) -> ServiceVars {
        ServiceVars {
            slug: slug.to_string(),
            name: slug.to_string(),
            env: env
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
        }
    }

    fn resolve(current_slug: &str, services: Vec<ServiceVars>) -> Result<EnvVars, String> {
        resolve_current_service_env(current_slug, services)
    }

    #[test]
    fn parses_same_service_and_cross_service_references() {
        let out = resolve(
            "api",
            vec![
                service("postgres", &[("DATABASE_URL", "postgres://db/app")]),
                service(
                    "api",
                    &[
                        ("LOCAL", "http://${{PORT}}"),
                        ("PORT", "3000"),
                        ("DB_URL", "${{postgres.DATABASE_URL}}"),
                    ],
                ),
            ],
        )
        .unwrap();

        assert_eq!(out["LOCAL"], "http://3000");
        assert_eq!(out["DB_URL"], "postgres://db/app");
    }

    #[test]
    fn resolves_nested_postgres_database_url() {
        let out = resolve(
            "api",
            vec![
                service(
                    "postgres",
                    &[
                        ("POSTGRES_USER", "postgres"),
                        ("POSTGRES_PASSWORD", "secret"),
                        ("POSTGRES_DB", "app"),
                        (
                            "DATABASE_URL",
                            "postgresql://${{POSTGRES_USER}}:${{POSTGRES_PASSWORD}}@${{DRIFTBASE_PRIVATE_HOSTNAME}}:5432/${{POSTGRES_DB}}",
                        ),
                    ],
                ),
                service("api", &[("DB_URL", "${{postgres.DATABASE_URL}}")]),
            ],
        )
        .unwrap();

        assert_eq!(
            out["DB_URL"],
            "postgresql://postgres:secret@postgres.driftbase.internal:5432/app"
        );
    }

    #[test]
    fn resolves_generated_private_hostname() {
        let out = resolve(
            "api",
            vec![service(
                "api",
                &[("HOST", "${{DRIFTBASE_PRIVATE_HOSTNAME}}")],
            )],
        )
        .unwrap();

        assert_eq!(out["HOST"], "api.driftbase.internal");
    }

    #[test]
    fn rejects_missing_service_and_variable() {
        let missing_service =
            resolve("api", vec![service("api", &[("A", "${{db.URL}}")])]).unwrap_err();
        assert!(missing_service.contains("unknown service reference 'db'"));

        let missing_variable = resolve(
            "api",
            vec![
                service("api", &[("A", "${{postgres.URL}}")]),
                service("postgres", &[]),
            ],
        )
        .unwrap_err();
        assert!(missing_variable.contains("unknown variable reference 'postgres.URL'"));

        let missing_local = resolve("api", vec![service("api", &[("A", "${{URL}}")])]).unwrap_err();
        assert!(missing_local.contains("unknown variable reference 'api.URL'"));
    }

    #[test]
    fn rejects_malformed_references() {
        let missing_close = resolve("api", vec![service("api", &[("A", "${{URL")])]).unwrap_err();
        assert!(missing_close.contains("missing closing"));

        let too_many_parts =
            resolve("api", vec![service("api", &[("A", "${{db.URL.EXTRA}}")])]).unwrap_err();
        assert!(too_many_parts.contains("malformed variable reference"));
    }

    #[test]
    fn rejects_cycles() {
        let local_cycle = resolve(
            "api",
            vec![service("api", &[("A", "${{B}}"), ("B", "${{A}}")])],
        )
        .unwrap_err();
        assert!(local_cycle.contains("cyclic variable reference"));

        let cross_cycle = resolve(
            "api",
            vec![
                service("api", &[("A", "${{worker.B}}")]),
                service("worker", &[("B", "${{api.A}}")]),
            ],
        )
        .unwrap_err();
        assert!(cross_cycle.contains("api.A -> worker.B -> api.A"));
    }

    #[test]
    fn rendering_does_not_mutate_raw_env() {
        let raw = EnvVars::from([(
            "DB_URL".to_string(),
            "${{postgres.DATABASE_URL}}".to_string(),
        )]);
        let out = resolve(
            "api",
            vec![
                service("postgres", &[("DATABASE_URL", "postgres://db/app")]),
                ServiceVars {
                    slug: "api".to_string(),
                    name: "api".to_string(),
                    env: raw.clone(),
                },
            ],
        )
        .unwrap();

        assert_eq!(raw["DB_URL"], "${{postgres.DATABASE_URL}}");
        assert_eq!(out["DB_URL"], "postgres://db/app");
    }
}
