use crate::dsn;
use postgres::types::{ToSql, Type};
use postgres::Client;
use std::convert::TryFrom;

// The table of the default workload: a row per client, which the client updates
pub const TABLE_NAME: &str = "pg_tps_optimizer";

pub struct Workload {
    dsn: dsn::Dsn,
    query: String,
    transactional: bool,
    prepared: bool,
}

impl Workload {
    pub fn new(dsn: dsn::Dsn, query: String, transactional: bool, prepared: bool) -> Workload {
        Workload {
            dsn,
            query,
            transactional,
            prepared,
        }
    }
    pub fn clone(&self) -> Workload {
        Workload {
            dsn: self.dsn.clone(),
            query: self.query.clone(),
            transactional: self.transactional,
            prepared: self.prepared,
        }
    }
    pub fn as_string(&self) -> String {
        format!(
            "dsn:{}\nquery: {}\ntransactional: {}\nprepared: {}",
            self.dsn.debug(),
            self.query(),
            self.transactional,
            self.prepared
        )
    }
    pub fn client(&self) -> Result<Client, Box<dyn std::error::Error>> {
        self.dsn.client()
    }
    // Without --query, the clients update their own row in TABLE_NAME
    pub fn is_default(&self) -> bool {
        self.query.trim().is_empty()
    }
    pub fn query(&self) -> String {
        match self.is_default() {
            true => format!("update {} set id=$1 where id=$1", TABLE_NAME),
            false => self.query.clone(),
        }
    }
    pub fn w_type(&self) -> WorkloadType {
        match (self.transactional, self.prepared) {
            (false, false) => WorkloadType::Default,
            (true, false) => WorkloadType::Transactional,
            (false, true) => WorkloadType::Prepared,
            (true, true) => WorkloadType::PreparedTransactional,
        }
    }
}

pub enum WorkloadType {
    Default,
    Transactional,
    Prepared,
    PreparedTransactional,
}

// The parameters for the query: the number of the client as $1, as the type PostgreSQL expects
// for $1 (from preparing the query), or nothing for a query without parameters
pub fn client_params(types: &[Type], id: u32) -> Result<Vec<Box<dyn ToSql + Sync>>, String> {
    let param: Box<dyn ToSql + Sync> = match types {
        [] => return Ok(Vec::new()),
        [t] if *t == Type::INT2 => Box::new(
            i16::try_from(id).map_err(|_| format!("client {} doesn't fit in a smallint", id))?,
        ),
        [t] if *t == Type::INT4 => Box::new(id as i32),
        [t] if *t == Type::INT8 => Box::new(id as i64),
        [t] if *t == Type::OID => Box::new(id),
        [t] if *t == Type::FLOAT4 => Box::new(id as f32),
        [t] if *t == Type::FLOAT8 => Box::new(id as f64),
        [t] if [
            Type::TEXT,
            Type::VARCHAR,
            Type::BPCHAR,
            Type::NAME,
            Type::UNKNOWN,
        ]
        .contains(t) =>
        {
            Box::new(id.to_string())
        }
        [t] => {
            return Err(format!(
            "$1 is the number of the client, but the query uses it as {}; cast it, like $1::int",
            t
        ))
        }
        _ => {
            return Err(format!(
                "the query has {} parameters, but only $1 (the number of the client) is supported",
                types.len()
            ))
        }
    };
    Ok(vec![param])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_params() {
        assert!(client_params(&[], 3).unwrap().is_empty());
        for t in [
            Type::INT2,
            Type::INT4,
            Type::INT8,
            Type::OID,
            Type::TEXT,
            Type::FLOAT8,
        ] {
            assert_eq!(
                client_params(std::slice::from_ref(&t), 3).unwrap().len(),
                1,
                "{}",
                t
            );
        }
        assert!(client_params(&[Type::INT2], 70000).is_err());
        assert!(client_params(&[Type::BOOL], 3)
            .unwrap_err()
            .contains("bool"));
        assert!(client_params(&[Type::INT4, Type::INT4], 3)
            .unwrap_err()
            .contains("2 parameters"));
    }
}
