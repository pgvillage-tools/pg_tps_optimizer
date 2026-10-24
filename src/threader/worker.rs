use crate::threader::sample::{ParallelSamples, Sample};
use chrono::Utc;
use postgres::{Client, Statement};
use std::sync::mpsc;
use std::thread;

use super::workload::{Workload, WorkloadType};

const TABLE_NAME: &str = "pg_tps_optimizer";

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
        let mut statement = prepare(&mut client, &self.workload.w_type())?;

        loop {
            if let Ok(done) = self.done.read() {
                // done is true when main thread decides we are there
                if *done {
                    break;
                }
            }
            match sample(
                &mut client,
                statement.as_ref(),
                self.workload.w_type(),
                (tps / 10_f64) as u64,
                self.id,
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
                            match prepare(&mut new_client, &self.workload.w_type()) {
                                Ok(new_statement) => {
                                    client = new_client;
                                    statement = new_statement;
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

fn update_query() -> String {
    format!("update {} set id=$1 where id=$1", TABLE_NAME)
}

// Prepared workloads prepare the statement once per connection, and run it for every query
fn prepare(
    client: &mut Client,
    w_type: &WorkloadType,
) -> Result<Option<Statement>, postgres::Error> {
    match w_type {
        WorkloadType::Prepared | WorkloadType::PreparedTransactional => {
            Ok(Some(client.prepare(&update_query())?))
        }
        WorkloadType::Default | WorkloadType::Transactional => Ok(None),
    }
}

fn sample(
    client: &mut Client,
    statement: Option<&Statement>,
    w_type: WorkloadType,
    mut num_queries: u64,
    thread_id: u32,
) -> Result<Sample, postgres::Error> {
    if num_queries < 1 {
        num_queries = 1;
    }
    let mut s = Sample::new();
    let query = update_query();

    for _x in 0..num_queries {
        let start = Utc::now();
        match (&w_type, statement) {
            (WorkloadType::Prepared, Some(statement)) => {
                client.query(statement, &[&thread_id])?;
            }
            (WorkloadType::PreparedTransactional, Some(statement)) => {
                let mut trans = client.transaction()?;
                trans.query(statement, &[&thread_id])?;
                trans.commit()?;
            }
            (WorkloadType::Transactional, _) => {
                let mut trans = client.transaction()?;
                trans.query(query.as_str(), &[&thread_id])?;
                trans.commit()?;
            }
            _ => {
                client.query(query.as_str(), &[&thread_id])?;
            }
        }
        s.increment(Utc::now() - start);
    }
    s.end();
    Ok(s)
}
