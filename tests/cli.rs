//! Tests that run the pg_tps_optimizer binary without PostgreSQL.

use std::process::{Command, Stdio};

// When the reader of stdout goes away (e.g. `pg_tps_optimizer | head`), the binary must stop
// quietly, like other command line tools, instead of panicking on the failed write
#[test]
fn stops_quietly_when_stdout_is_closed() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pg_tps_optimizer"))
        // Nothing to connect to: the output before connecting is enough
        .env("PGHOST", "/nonexistent")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("could not start pg_tps_optimizer");
    // Close the read end of stdout before the binary writes to it
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked") && output.status.code() != Some(101),
        "pg_tps_optimizer panicked on a closed stdout ({})\nstderr:\n{}",
        output.status,
        stderr
    );
}

// --graph with --graph-input draws the graph of earlier JSON reports, without connecting
#[test]
fn draws_graph_from_json_reports() {
    let dir =
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("draws_graph_from_json_reports");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let step = |clients: u32, tps: f64| {
        format!(
            r#"{{"timestamp": "2026-10-24T10:00:00+02:00", "clients": {}, "stable": true,
                "tps": {}, "latency_us": 500.0, "tps_per_latency": 2.0, "postgres_tps": {},
                "wal_kb_per_sec": 100.0}}"#,
            clients, tps, tps
        )
    };
    for (name, factor) in [("run1", 1.0), ("run2", 1.5)] {
        let steps: Vec<String> = [(1, 1000.0), (2, 1800.0), (3, 2400.0), (5, 2200.0)]
            .iter()
            .map(|(clients, tps)| step(*clients, tps * factor))
            .collect();
        std::fs::write(
            dir.join(format!("{}.json", name)),
            format!(r#"{{"version": "0.0.0", "steps": [{}]}}"#, steps.join(",")),
        )
        .unwrap();
    }
    let graph = dir.join("graph.png");
    let output = Command::new(env!("CARGO_BIN_EXE_pg_tps_optimizer"))
        // Nothing to connect to: drawing the graph must not need PostgreSQL
        .env("PGHOST", "/nonexistent")
        .arg("--graph")
        .arg(&graph)
        .arg("--graph-input")
        .arg(dir.join("run1.json"))
        .arg("--graph-input")
        .arg(dir.join("run2.json"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let png = std::fs::read(&graph).unwrap();
    assert_eq!(&png[1..4], b"PNG");

    // SVG for a .svg file name
    let svg_graph = dir.join("graph.svg");
    let output = Command::new(env!("CARGO_BIN_EXE_pg_tps_optimizer"))
        .arg("--graph")
        .arg(&svg_graph)
        .arg("--graph-input")
        .arg(dir.join("run1.json"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let svg = std::fs::read_to_string(&svg_graph).unwrap();
    assert!(svg.starts_with("<svg") && svg.contains("run1"), "{}", svg);

    // A file that is not a JSON report gives a clean error
    std::fs::write(dir.join("bad.json"), "not json").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_pg_tps_optimizer"))
        .arg("--graph")
        .arg(&graph)
        .arg("--graph-input")
        .arg(dir.join("bad.json"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not a JSON report"));
}
