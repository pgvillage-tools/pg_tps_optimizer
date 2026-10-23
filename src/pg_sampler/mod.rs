/*
Pg_sampler can be used to periodically get statistics information from PostgreSQL,
The main idea is to get the number of transactions and de amount of WAL.
We also capture the duration between 2 samples, and as such also know TPS and WAL per sec.
*/
use crate::dsn::Dsn;
use chrono::Utc;
use postgres::{Client, Error, Statement};

const SAMPLE_QUERY: &str = "
SELECT now()::timestamp as samplemmoment,
pg_current_wal_lsn()::varchar as lsn,
(pg_current_wal_lsn() - $1::varchar::pg_lsn)::real as walbytes,
(select sum(xact_commit+xact_rollback)::real
 FROM pg_stat_database) as transacts";

// This struct can run a query against postgres and see
pub struct PgSampler {
    client: Client,
    statement: Statement,
    previous: TransactDataSample,
    latest: TransactDataSample,
}

impl PgSampler {
    pub fn new(dsn: Dsn) -> Result<PgSampler, Box<dyn std::error::Error>> {
        let mut client: Client = dsn.client()?;
        let statement: Statement = client.prepare(SAMPLE_QUERY)?;
        Ok(PgSampler {
            client,
            statement,
            previous: TransactDataSample::new(),
            latest: TransactDataSample::new(),
        })
    }
    pub fn next(&mut self) -> Result<(), Error> {
        let rows = self.client.query(&self.statement, &[&self.previous.lsn])?;
        assert_eq!(rows.len(), 1);
        let row = rows.first().unwrap();
        self.previous = self.latest.clone();
        self.latest = TransactDataSample {
            samplemoment: row.get(0),
            lsn: row.get(1),
            wal_bytes: row.get(2),
            num_transactions: row.get(3),
        };
        Ok(())
    }
    pub fn duration(&self) -> f32 {
        (self.latest.samplemoment - self.previous.samplemoment)
            .num_nanoseconds()
            .unwrap() as f32
            / 1.0e+9_f32
    }
    pub fn wal_per_sec(&self) -> f32 {
        let wps = (self.latest.wal_bytes - self.previous.wal_bytes) / self.duration();
        if wps < 0.0 {
            return -1.0;
        }
        wps
    }
    pub fn tps(&self) -> f32 {
        (self.latest.num_transactions - self.previous.num_transactions) / self.duration()
    }
}

struct TransactDataSample {
    samplemoment: chrono::NaiveDateTime,
    lsn: String,
    wal_bytes: f32,
    num_transactions: f32,
}

impl TransactDataSample {
    fn new() -> TransactDataSample {
        TransactDataSample {
            samplemoment: Utc::now().naive_utc(),
            lsn: "0/0".to_string(),
            wal_bytes: 0.0_f32,
            num_transactions: 0.0_f32,
        }
    }
    fn clone(&self) -> TransactDataSample {
        TransactDataSample {
            samplemoment: self.samplemoment,
            lsn: self.lsn.clone(),
            wal_bytes: self.wal_bytes,
            num_transactions: self.num_transactions,
        }
    }
}
