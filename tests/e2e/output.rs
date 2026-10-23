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
            ("PGTPSRANGE", "1:4".to_string()),
        ],
    );
    assert!(
        !run.timed_out && run.output.status.success(),
        "{}",
        run.describe()
    );
    let report: serde_json::Value = serde_json::from_slice(&run.output.stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({}): {}", e, run.describe()));
    assert_eq!(report["settings"]["max_clients"], 4);
    let steps = report["steps"].as_array().unwrap();
    // Fibonacci steps below 4 clients: 1, 2, 3
    let clients: Vec<u64> = steps
        .iter()
        .map(|s| s["clients"].as_u64().unwrap())
        .collect();
    assert_eq!(clients, vec![1, 2, 3], "{}", run.describe());
    for step in steps {
        assert!(step["tps"].as_f64().unwrap() > 0.0, "{}", step);
        assert!(step["latency_us"].as_f64().unwrap() > 0.0, "{}", step);
    }
    // The progress messages go to stderr
    assert!(String::from_utf8_lossy(&run.output.stderr).contains("Finished"));
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
