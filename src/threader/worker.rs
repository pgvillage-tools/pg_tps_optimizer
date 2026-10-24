use crate::threader::sample::{ParallelSamples, Sample};
use chrono::Utc;
use postgres::types::ToSql;
use postgres::{Client, Statement};
use std::sync::mpsc;
use std::thread;

use super::workload::{client_params, Workload, WorkloadType, TABLE_NAME};

pub struct Worker {
    id: u32,
    tx: mpsc::Sender<ParallelSamples>,
    done: std::sync::Arc<std::sync::RwLock<bool>>,
    workload: Workload,
}

impl Worker {
    pub fn new(
        id: u32,
        tx: mpsc::Sender<ParallelSamples>,
        done: std::sync::Arc<std::sync::RwLock<bool>>,
        workload: Workload,
    ) -> Worker {
        //println!("Started new worker: {}", id);
        Worker {
            id,
            tx,
            done,
            workload,
        }
    }
    pub fn initialize(&self) -> Result<Client, Box<dyn std::error::Error>> {
        let mut client = self.workload.client()?;
        // The default workload updates a row per client; a custom query needs no setup
        if !self.workload.is_default() {
            return Ok(client);
        }
        client.query(
            format!("create table if not exists {} (id oid)", TABLE_NAME).as_str(),
            &[],
        )?;
        if self.id == 0 {
            client.query(format!("truncate table {}", TABLE_NAME).as_str(), &[])?;
        }
        client.query(
            format!("insert into {} values($1)", TABLE_NAME).as_str(),
            &[&self.id],
        )?;

        Ok(client)
    }
    pub fn procedure(self) -> Result<(), Box<dyn std::error::Error>> {
        let mut tps: f64 = 1000_f64;

        //Sleep 100 milliseconds
        let mut client = self.initialize()?;
        let mut session = Session::new(&mut client, &self.workload, self.id)?;

        loop {
            if let Ok(done) = self.done.read() {
                // done is true when main thread decides we are there
                if *done {
                    break;
                }
            }
            match sample(
                &mut client,
                &session,
                self.workload.w_type(),
                (tps / 10_f64) as u64,
            ) {
                Ok(sample) => {
                    //tps = samples.tot_tps_singlethread() as u64;
                    let mut pss = ParallelSamples::new();
                    pss.add(sample.to_parallel_sample());
                    self.tx.send(pss)?;
                    tps = sample.tps();
                }
                Err(err) => {
                    eprintln!("error: worker {}: {}", self.id, err);
                    let sleeptime = std::time::Duration::from_millis(100);
                    thread::sleep(sleeptime);
                    // A prepared statement belongs to its connection, so prepare it again
                    match self.workload.client() {
                        Ok(mut new_client) => {
                            match Session::new(&mut new_client, &self.workload, self.id) {
                                Ok(new_session) => {
                                    client = new_client;
                                    session = new_session;
                                }
                                Err(err) => eprintln!("error: worker {}: {}", self.id, err),
                            }
                        }
                        Err(err) => eprintln!("error: worker {}: {}", self.id, err),
                    }
                }
            };
        }
        Ok(())
    }
}

// What a client needs to run the query on its connection: the query, the statement (for
// --prepared, prepared once per connection), and the parameters
struct Session {
    query: String,
    statement: Option<Statement>,
    params: Vec<Box<dyn ToSql + Sync>>,
}

impl Session {
    fn new(
        client: &mut Client,
        workload: &Workload,
        id: u32,
    ) -> Result<Session, Box<dyn std::error::Error>> {
        let query = workload.query();
        // Preparing tells which type PostgreSQL expects for $1
        let statement = client.prepare(&query)?;
        let params = client_params(statement.params(), id)?;
        let statement = match workload.w_type() {
            WorkloadType::Prepared | WorkloadType::PreparedTransactional => Some(statement),
            WorkloadType::Default | WorkloadType::Transactional => None,
        };
        Ok(Session {
            query,
            statement,
            params,
        })
    }
    fn params(&self) -> Vec<&(dyn ToSql + Sync)> {
        self.params.iter().map(|param| param.as_ref()).collect()
    }
}

fn sample(
    client: &mut Client,
    session: &Session,
    w_type: WorkloadType,
    mut num_queries: u64,
) -> Result<Sample, postgres::Error> {
    if num_queries < 1 {
        num_queries = 1;
    }
    let mut s = Sample::new();
    let params = session.params();

    for _x in 0..num_queries {
        let start = Utc::now();
        match (&w_type, &session.statement) {
            (WorkloadType::Prepared, Some(statement)) => {
                client.query(statement, &params)?;
            }
            (WorkloadType::PreparedTransactional, Some(statement)) => {
                let mut trans = client.transaction()?;
                trans.query(statement, &params)?;
                trans.commit()?;
            }
            (WorkloadType::Transactional, _) => {
                let mut trans = client.transaction()?;
                trans.query(session.query.as_str(), &params)?;
                trans.commit()?;
            }
            _ => {
                client.query(session.query.as_str(), &params)?;
            }
        }
        s.increment(Utc::now() - start);
    }
    s.end();
    Ok(s)
}
