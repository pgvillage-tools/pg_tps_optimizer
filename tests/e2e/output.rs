// Output formats: JSON (and the graph)
use crate::common::*;

// With --output json, stdout only has a JSON document with the settings and the steps
#[test]
#[ignore = "requires a container runtime"]
fn outputs_json() {
    let pg = start_postgres();
    let home = scratch_dir("outputs_json").join("home");
    fs::create_dir_all(&home).unwrap();
    let run = run_pg_tps_optimizer(
        &home,
        &[
            ("PGHOST", "localhost".to_string()),
            ("PGPORT", pg.get_host_port_ipv4(5432).unwrap().to_string()),
            ("PGUSER", "postgres".to_string()),
            ("PGDATABASE", "postgres".to_string()),
            ("PGSSLMODE", "disable".to_string()),
            ("PGTPSOUTPUT", "json".to_string()),
            ("PGTPSRANGE", "1:3".to_string()),
            ("PGTPSMAXWAIT", "2500ms".to_string()),
        ],
    );
    assert!(
        !run.timed_out && run.output.status.success(),
        "{}",
        run.describe()
    );
    let report: serde_json::Value = serde_json::from_slice(&run.output.stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({}): {}", e, run.describe()));
    assert_eq!(report["settings"]["max_clients"], 3);
    assert_eq!(report["settings"]["max_wait_seconds"], 2.5);
    assert_eq!(
        report["settings"]["query"],
        "update pg_tps_optimizer set id=$1 where id=$1"
    );
    let steps = report["steps"].as_array().unwrap();
    // Fibonacci steps up to 3 clients, the maximum included: 1, 2, 3
    let clients: Vec<u64> = steps
        .iter()
        .map(|s| s["clients"].as_u64().unwrap())
        .collect();
    assert_eq!(clients, vec![1, 2, 3], "{}", run.describe());
    for step in steps {
        assert!(step["tps"].as_f64().unwrap() > 0.0, "{}", step);
        assert!(step["latency_us"].as_f64().unwrap() > 0.0, "{}", step);
    }
    // The progress messages go to stderr, with the settings
    let stderr = String::from_utf8_lossy(&run.output.stderr).to_string();
    assert!(
        stderr.contains("Finished")
            && stderr.contains("query: update pg_tps_optimizer set id=$1 where id=$1")
            && stderr.contains("max_wait: 2.5s"),
        "{}",
        run.describe()
    );
}

// --graph writes a PNG with the TPS per number of clients of the run
#[test]
#[ignore = "requires a container runtime"]
fn writes_graph() {
    let pg = start_postgres();
    let dir = scratch_dir("writes_graph");
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let graph = dir.join("graph.png");
    run_pg_tps_optimizer(
        &home,
        &[
            ("PGHOST", "localhost".to_string()),
            ("PGPORT", pg.get_host_port_ipv4(5432).unwrap().to_string()),
            ("PGUSER", "postgres".to_string()),
            ("PGDATABASE", "postgres".to_string()),
            ("PGSSLMODE", "disable".to_string()),
            ("PGTPSRANGE", "1:4".to_string()),
            ("PGTPSGRAPH", graph.display().to_string()),
        ],
    )
    .assert_success();
    let png = fs::read(&graph).unwrap();
    assert_eq!(&png[1..4], b"PNG");
}
