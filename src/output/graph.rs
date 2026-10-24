// Draws a graph (PNG or SVG) with the TPS per number of clients: one line per run
use super::Step;
use clap::ValueEnum;
use plotters::coord::Shift;
use plotters::prelude::*;
use plotters::style::{register_font, FontStyle};
use std::error::Error;
use std::path::Path;
use std::sync::Once;

#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
pub enum GraphFormat {
    /// SVG when the file name ends with .svg, otherwise PNG
    Auto,
    Png,
    Svg,
}

impl GraphFormat {
    fn resolve(self, path: &Path) -> GraphFormat {
        match self {
            GraphFormat::Auto => match path.extension() {
                Some(ext) if ext.eq_ignore_ascii_case("svg") => GraphFormat::Svg,
                _ => GraphFormat::Png,
            },
            format => format,
        }
    }
}

const SIZE: (u32, u32) = (900, 600);

pub struct Series {
    pub name: String,
    pub steps: Vec<Step>,
}

// The font is built into the binary, as the (static) binaries can't rely on system fonts
fn register_fonts() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        // DejaVu Sans is a valid font, so registering it can't fail
        let _ = register_font("sans-serif", FontStyle::Normal, dejavu::sans::regular());
    });
}

pub fn draw(path: &Path, format: GraphFormat, series: &[Series]) -> Result<(), Box<dyn Error>> {
    // Also an SVG needs the font: to measure the text for the layout
    register_fonts();
    match format.resolve(path) {
        GraphFormat::Svg => draw_on(&SVGBackend::new(path, SIZE).into_drawing_area(), series),
        _ => draw_on(&BitMapBackend::new(path, SIZE).into_drawing_area(), series),
    }
}

fn draw_on<DB: DrawingBackend>(
    root: &DrawingArea<DB, Shift>,
    series: &[Series],
) -> Result<(), Box<dyn Error>>
where
    DB::ErrorType: 'static,
{
    // The numbers of clients of all runs, evenly spaced on the x axis (like 1, 2, 3, 5, 8, ...)
    let mut clients: Vec<u32> = series
        .iter()
        .flat_map(|s| s.steps.iter().map(|step| step.clients))
        .collect();
    clients.sort_unstable();
    clients.dedup();
    if clients.is_empty() {
        return Err("no results to draw a graph of".into());
    }
    let max_tps = series
        .iter()
        .flat_map(|s| s.steps.iter().filter_map(|step| step.tps))
        .fold(0.0_f64, f64::max);

    root.fill(&WHITE)?;
    let mut chart = ChartBuilder::on(root)
        .caption("Measured TPS over number of clients", ("sans-serif", 24))
        .margin(15)
        .x_label_area_size(50)
        .y_label_area_size(80)
        .build_cartesian_2d(
            -0.5_f64..(clients.len() as f64 - 0.5),
            0.0_f64..(max_tps * 1.1).max(1.0),
        )?;
    chart
        .configure_mesh()
        .x_desc("Number of clients")
        .y_desc("Transactions per second")
        .x_labels(clients.len())
        .x_label_formatter(&|x| {
            let index = x.round();
            match (index - x).abs() < 0.01 && index >= 0.0 {
                true => clients
                    .get(index as usize)
                    .map(|c| c.to_string())
                    .unwrap_or_default(),
                false => String::new(),
            }
        })
        .y_label_formatter(&|y| format!("{:.0}", y))
        .label_style(("sans-serif", 14))
        .axis_desc_style(("sans-serif", 16))
        .draw()?;

    for (index, run) in series.iter().enumerate() {
        let color = Palette99::pick(index).to_rgba();
        let points: Vec<(f64, f64)> = run
            .steps
            .iter()
            .filter_map(|step| {
                let x = clients.iter().position(|c| *c == step.clients)? as f64;
                Some((x, step.tps?))
            })
            .collect();
        chart
            .draw_series(LineSeries::new(points.clone(), color.stroke_width(2)))?
            .label(&run.name)
            .legend(move |(x, y)| {
                PathElement::new(vec![(x, y), (x + 20, y)], color.stroke_width(2))
            });
        chart.draw_series(
            points
                .into_iter()
                .map(|p| Circle::new(p, 3, color.filled())),
        )?;
    }
    chart
        .configure_series_labels()
        .position(SeriesLabelPosition::UpperLeft)
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .label_font(("sans-serif", 14))
        .draw()?;
    root.present()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(clients: u32, tps: Option<f64>) -> Step {
        Step {
            timestamp: String::new(),
            clients,
            stable: true,
            tps,
            latency_us: None,
            tps_per_latency: None,
            postgres_tps: None,
            wal_kb_per_sec: None,
        }
    }

    #[test]
    fn test_draw() {
        let file = std::env::temp_dir().join(format!("pg_tps_graph_{}.png", std::process::id()));
        let series = vec![
            Series {
                name: "run 1".to_string(),
                steps: vec![step(1, Some(1000.0)), step(2, Some(1800.0)), step(3, None)],
            },
            Series {
                name: "run 2".to_string(),
                steps: vec![step(1, Some(900.0)), step(5, Some(2500.0))],
            },
        ];
        draw(&file, GraphFormat::Auto, &series).unwrap();
        let png = std::fs::read(&file).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        std::fs::remove_file(&file).unwrap();

        // SVG by extension, or explicitly
        let svg_file = file.with_extension("svg");
        draw(&svg_file, GraphFormat::Auto, &series).unwrap();
        let svg = std::fs::read_to_string(&svg_file).unwrap();
        assert!(svg.starts_with("<svg") && svg.contains("run 2"), "{}", svg);
        std::fs::remove_file(&svg_file).unwrap();
        draw(&file, GraphFormat::Svg, &series).unwrap();
        assert!(std::fs::read_to_string(&file).unwrap().starts_with("<svg"));
        std::fs::remove_file(&file).unwrap();

        let err = draw(&file, GraphFormat::Auto, &[]).unwrap_err();
        assert_eq!(err.to_string(), "no results to draw a graph of");
    }
}
