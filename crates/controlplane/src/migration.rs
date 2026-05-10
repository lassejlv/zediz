use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(InitialSchema)]
    }
}

#[derive(DeriveMigrationName)]
struct InitialSchema;

#[async_trait::async_trait]
impl MigrationTrait for InitialSchema {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let schema = strip_line_comments(include_str!("migration/initial_schema.sql"));
        for statement in split_sql_statements(&schema) {
            db.execute_unprepared(statement).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for statement in split_sql_statements(DOWN_SQL) {
            db.execute_unprepared(statement).await?;
        }
        Ok(())
    }
}

fn split_sql_statements(sql: &str) -> impl Iterator<Item = &str> {
    sql.split(';')
        .map(str::trim)
        .filter(|stmt| !stmt.is_empty())
}

fn strip_line_comments(sql: &str) -> String {
    let mut stripped = String::with_capacity(sql.len());
    for line in sql.lines() {
        let line = line.split_once("--").map_or(line, |(before, _)| before);
        stripped.push_str(line);
        stripped.push('\n');
    }
    stripped
}

const DOWN_SQL: &str = r#"
DROP TABLE IF EXISTS project_network_node_subnets CASCADE;
DROP TABLE IF EXISTS project_networks CASCADE;
DROP TABLE IF EXISTS deployment_metrics CASCADE;
DROP TABLE IF EXISTS volumes CASCADE;
DROP TABLE IF EXISTS builds CASCADE;
DROP TABLE IF EXISTS service_domains CASCADE;
DROP TABLE IF EXISTS deployment_logs CASCADE;
DROP TABLE IF EXISTS agent_commands CASCADE;
DROP TABLE IF EXISTS node_allocations CASCADE;
DROP TABLE IF EXISTS deployments CASCADE;
DROP TABLE IF EXISTS nodes CASCADE;
DROP TABLE IF EXISTS services CASCADE;
DROP TABLE IF EXISTS projects CASCADE;
DROP TABLE IF EXISTS ssh_keys CASCADE;
DROP TABLE IF EXISTS credentials CASCADE;
DROP TABLE IF EXISTS invites CASCADE;
DROP TABLE IF EXISTS workspace_members CASCADE;
DROP TABLE IF EXISTS workspaces CASCADE;
DROP TABLE IF EXISTS sessions CASCADE;
DROP TABLE IF EXISTS users CASCADE;
DROP EXTENSION IF EXISTS citext;
"#;
