use crate::dsn;
use postgres::Client;

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
            "dsn:{}\ntransactional: {}\nprepared: {}",
            self.dsn.debug(),
            self.transactional,
            self.prepared
        )
    }
    pub fn client(&self) -> Result<Client, Box<dyn std::error::Error>> {
        self.dsn.client()
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
