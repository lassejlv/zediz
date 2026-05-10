use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Value;
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, DbErr,
    FromQueryResult, QueryResult, SqlErr, Statement, TryGetError, TryGetableMany,
};
use serde_json::Value as JsonValue;

pub type Db = DatabaseConnection;

pub async fn connect(database_url: &str) -> Result<Db> {
    let mut options = ConnectOptions::new(database_url.to_string());
    options
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .sqlx_logging(false);

    Database::connect(options)
        .await
        .context("connecting to postgres")
}

pub async fn migrate(db: &Db) -> Result<()> {
    use sea_orm_migration::MigratorTrait;

    crate::migration::Migrator::up(db, None)
        .await
        .context("running migrations")?;
    Ok(())
}

pub fn query(sql: impl Into<String>) -> RawQuery {
    RawQuery::new(sql)
}

pub fn query_as<T>(sql: impl Into<String>) -> RawQueryAs<T> {
    RawQueryAs::new(sql)
}

pub fn query_tuple<T>(sql: impl Into<String>) -> RawQueryTuple<T> {
    RawQueryTuple::new(sql)
}

pub fn is_unique_violation(err: &DbErr) -> bool {
    matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_)))
}

pub fn is_not_found(err: &DbErr) -> bool {
    matches!(err, DbErr::RecordNotFound(_))
}

pub struct RawQuery {
    sql: String,
    values: Vec<Value>,
}

impl RawQuery {
    fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            values: Vec::new(),
        }
    }

    pub fn bind(mut self, value: impl BindValue) -> Self {
        self.values.push(value.into_value());
        self
    }

    pub async fn execute<C>(self, db: &C) -> Result<sea_orm::ExecResult, DbErr>
    where
        C: ConnectionTrait,
    {
        db.execute(self.statement()).await
    }

    fn statement(self) -> Statement {
        Statement::from_sql_and_values(DbBackend::Postgres, self.sql, self.values)
    }
}

pub struct RawQueryAs<T> {
    inner: RawQuery,
    _ty: std::marker::PhantomData<T>,
}

impl<T> RawQueryAs<T> {
    fn new(sql: impl Into<String>) -> Self {
        Self {
            inner: RawQuery::new(sql),
            _ty: std::marker::PhantomData,
        }
    }

    pub fn bind(mut self, value: impl BindValue) -> Self {
        self.inner = self.inner.bind(value);
        self
    }
}

impl<T> RawQueryAs<T>
where
    T: FromQueryResult + Send + Sync + 'static,
{
    pub async fn fetch_all<C>(self, db: &C) -> Result<Vec<T>, DbErr>
    where
        C: ConnectionTrait,
    {
        let rows = db.query_all(self.inner.statement()).await?;
        rows.iter()
            .map(|row| T::from_query_result(row, ""))
            .collect()
    }

    pub async fn fetch_optional<C>(self, db: &C) -> Result<Option<T>, DbErr>
    where
        C: ConnectionTrait,
    {
        let row = db.query_one(self.inner.statement()).await?;
        row.map(|row| T::from_query_result(&row, "")).transpose()
    }

    pub async fn fetch_one<C>(self, db: &C) -> Result<T, DbErr>
    where
        C: ConnectionTrait,
    {
        self.fetch_optional(db)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound("row not found".to_string()))
    }
}

pub struct RawQueryTuple<T> {
    inner: RawQuery,
    _ty: std::marker::PhantomData<T>,
}

impl<T> RawQueryTuple<T> {
    fn new(sql: impl Into<String>) -> Self {
        Self {
            inner: RawQuery::new(sql),
            _ty: std::marker::PhantomData,
        }
    }

    pub fn bind(mut self, value: impl BindValue) -> Self {
        self.inner = self.inner.bind(value);
        self
    }
}

impl<T> RawQueryTuple<T>
where
    T: TryGetableMany + Send + Sync + 'static,
{
    pub async fn fetch_all<C>(self, db: &C) -> Result<Vec<T>, DbErr>
    where
        C: ConnectionTrait,
    {
        let rows = db.query_all(self.inner.statement()).await?;
        rows.iter()
            .map(tuple_from_query_result::<T>)
            .collect::<Result<Vec<_>, _>>()
    }

    pub async fn fetch_optional<C>(self, db: &C) -> Result<Option<T>, DbErr>
    where
        C: ConnectionTrait,
    {
        let row = db.query_one(self.inner.statement()).await?;
        row.map(|row| tuple_from_query_result::<T>(&row))
            .transpose()
    }

    pub async fn fetch_one<C>(self, db: &C) -> Result<T, DbErr>
    where
        C: ConnectionTrait,
    {
        self.fetch_optional(db)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound("row not found".to_string()))
    }
}

fn tuple_from_query_result<T>(row: &QueryResult) -> Result<T, DbErr>
where
    T: TryGetableMany,
{
    T::try_get_many_by_index(row).map_err(|err| match err {
        TryGetError::DbErr(err) => err,
        TryGetError::Null(column) => DbErr::Type(format!("null value in column {column}")),
    })
}

pub trait BindValue {
    fn into_value(self) -> Value;
}

macro_rules! impl_bind_into {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl BindValue for $ty {
                fn into_value(self) -> Value {
                    self.into()
                }
            }
        )+
    };
}

impl_bind_into!(
    bool,
    i8,
    i16,
    i32,
    i64,
    u8,
    u16,
    u32,
    u64,
    f32,
    f64,
    String,
    JsonValue,
    Vec<u8>,
    DateTime<Utc>,
    Option<bool>,
    Option<i8>,
    Option<i16>,
    Option<i32>,
    Option<i64>,
    Option<u8>,
    Option<u16>,
    Option<u32>,
    Option<u64>,
    Option<f32>,
    Option<f64>,
    Option<String>,
    Option<JsonValue>,
    Option<Vec<u8>>,
    Option<DateTime<Utc>>,
);

impl<T> BindValue for &T
where
    T: Clone + Into<Value>,
{
    fn into_value(self) -> Value {
        self.clone().into()
    }
}

impl BindValue for &str {
    fn into_value(self) -> Value {
        self.into()
    }
}

impl BindValue for &[u8] {
    fn into_value(self) -> Value {
        self.into()
    }
}

impl BindValue for Option<&String> {
    fn into_value(self) -> Value {
        self.map(|value| value.as_str()).into()
    }
}

impl BindValue for Option<&str> {
    fn into_value(self) -> Value {
        self.into()
    }
}

impl BindValue for Option<&JsonValue> {
    fn into_value(self) -> Value {
        match self {
            Some(value) => value.clone().into(),
            None => Value::Json(None),
        }
    }
}

impl BindValue for Option<&[u8]> {
    fn into_value(self) -> Value {
        match self {
            Some(value) => value.into(),
            None => Value::Bytes(None),
        }
    }
}

impl BindValue for Option<&Vec<u8>> {
    fn into_value(self) -> Value {
        match self {
            Some(value) => value.as_slice().into(),
            None => Value::Bytes(None),
        }
    }
}
