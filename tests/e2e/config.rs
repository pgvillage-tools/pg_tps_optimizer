// The config file (--config)
use crate::common::*;

// A config file (--config / PGTPSCONFIG) sets the options; environment variables win
#[test]
#[ignore = "requires a container runtime"]
fn uses_config_file() {
    let pg = start_postgres();
    let dir = scratch_dir("uses_config_file");
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let config = dir.join("pg_tps_optimizer.yaml");
    fs::write(
        &config,
        format!(
            "# Settings for the e2e test\n\
             dsn: host=localhost port={} user=postgres dbname=postgres sslmode=disable\n\
             range: \"1:100\"\n\
             spread: 7\n\
             min-samples: 5\n",
            pg.get_host_port_ipv4(5432).unwrap()
        ),
    )
    .unwrap();
    // run_pg_tps_optimizer sets PGTPSRANGE=1:2, which wins over the range in the config file
    let run = run_pg_tps_optimizer(&home, &[("PGTPSCONFIG", config.display().to_string())]);
    run.assert_success();
    let stdout = String::from_utf8_lossy(&run.output.stdout);
    assert!(stdout.contains("max threads: 2"), "{}", run.describe());
    assert!(
        stdout.contains("min_samples: 5, spread: 7"),
        "{}",
        run.describe()
    );

    // An unknown option is an error
    fs::write(&config, "rnage: 1:5\n").unwrap();
    let run = run_pg_tps_optimizer(&home, &[("PGTPSCONFIG", config.display().to_string())]);
    assert_eq!(run.output.status.code(), Some(2), "{}", run.describe());
    assert!(String::from_utf8_lossy(&run.output.stderr).contains("unknown option \"rnage\""));
}
