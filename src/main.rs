use std::env;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
struct Params {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

struct Config {
    width: usize,
    height: usize,
    iterations: u64,
    warmup: u64,
    cutoff_x: f64,
    cutoff_y: f64,
    exposure: Option<f64>,
    palette: String,
    progress: bool,
    threads: usize,
    out_path: PathBuf,
}

#[derive(Clone, Copy)]
struct Mapper {
    width: usize,
    height: usize,
    scale_x: f64,
    scale_y: f64,
    offset_x: f64,
    offset_y: f64,
}

struct Progress {
    label: &'static str,
    total: u64,
    current: u64,
    started_at: Instant,
    last_printed_at: Instant,
}

#[derive(Clone, Copy)]
struct Rgb {
    r: f64,
    g: f64,
    b: f64,
}

type Palette = &'static [(f64, Rgb)];
type Count = u16;

const DUSK: Palette = &[
    (0.00, rgb(7, 10, 18)),
    (0.18, rgb(20, 62, 88)),
    (0.48, rgb(39, 145, 157)),
    (0.78, rgb(242, 177, 82)),
    (1.00, rgb(255, 247, 214)),
];

const EMBER: Palette = &[
    (0.00, rgb(8, 7, 10)),
    (0.24, rgb(73, 18, 38)),
    (0.55, rgb(184, 58, 45)),
    (0.82, rgb(250, 159, 69)),
    (1.00, rgb(255, 237, 190)),
];

const AURORA: Palette = &[
    (0.00, rgb(4, 9, 20)),
    (0.22, rgb(46, 36, 112)),
    (0.48, rgb(40, 150, 177)),
    (0.75, rgb(111, 220, 154)),
    (1.00, rgb(249, 246, 189)),
];

const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb {
        r: r as f64,
        g: g as f64,
        b: b as f64,
    }
}

impl Progress {
    fn new(label: &'static str, total: u64, enabled: bool) -> Option<Self> {
        if !enabled {
            return None;
        }

        let now = Instant::now();
        let progress = Self {
            label,
            total,
            current: 0,
            started_at: now,
            last_printed_at: now - Duration::from_secs(1),
        };
        progress.print();
        Some(progress)
    }

    fn set(&mut self, current: u64) {
        self.current = current.min(self.total);
        if self.last_printed_at.elapsed() >= Duration::from_millis(250)
            || self.current == self.total
        {
            self.print();
            self.last_printed_at = Instant::now();
        }
    }

    fn finish(mut self) {
        if self.current < self.total {
            self.current = self.total;
            self.print();
        }
        eprintln!();
    }

    fn print(&self) {
        let percent = if self.total == 0 {
            100.0
        } else {
            self.current as f64 / self.total as f64 * 100.0
        };
        let elapsed = self.started_at.elapsed().as_secs_f64();
        let rate = if elapsed <= 0.0 {
            0.0
        } else {
            self.current as f64 / elapsed
        };
        eprint!(
            "\r{:<9} {:>6.2}% {}/{} {:>8.2}M/s",
            self.label,
            percent,
            format_count(self.current),
            format_count(self.total),
            rate / 1_000_000.0
        );
        let _ = io::stderr().flush();
    }
}

impl Mapper {
    fn new(config: &Config) -> Self {
        let scale_x = config.width as f64 / config.cutoff_x;
        let scale_y = config.height as f64 / config.cutoff_y;
        Self {
            width: config.width,
            height: config.height,
            scale_x,
            scale_y,
            offset_x: config.cutoff_x * 0.5 * scale_x,
            offset_y: config.cutoff_y * 0.5 * scale_y,
        }
    }
}

fn format_count(value: u64) -> String {
    if value >= 1_000_000_000 {
        format!("{:.2}B", value as f64 / 1_000_000_000.0)
    } else if value >= 1_000_000 {
        format!("{:.2}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.2}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn palette_by_name(name: &str) -> Result<Palette, String> {
    match name.to_ascii_lowercase().as_str() {
        "dusk" => Ok(DUSK),
        "ember" => Ok(EMBER),
        "aurora" => Ok(AURORA),
        _ => Err(format!(
            "unknown palette {name:?}; available palettes: dusk, ember, aurora"
        )),
    }
}

fn clamp_byte(value: f64) -> u8 {
    value.clamp(0.0, 255.0).round() as u8
}

fn mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn color_at(palette: Palette, t: f64) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let mut left = palette[0];
    let mut right = palette[palette.len() - 1];

    for window in palette.windows(2) {
        if t <= window[1].0 {
            left = window[0];
            right = window[1];
            break;
        }
    }

    let span = right.0 - left.0;
    let local_t = if span <= 0.0 {
        0.0
    } else {
        (t - left.0) / span
    };
    [
        clamp_byte(mix(left.1.r, right.1.r, local_t)),
        clamp_byte(mix(left.1.g, right.1.g, local_t)),
        clamp_byte(mix(left.1.b, right.1.b, local_t)),
    ]
}

fn params() -> Params {
    Params {
        a: -1.7,
        b: 1.8,
        c: -0.9,
        d: -0.4,
    }
}

#[inline(always)]
fn fast_sin_cos(x: f64) -> (f64, f64) {
    const FRAC_2_PI: f64 = std::f64::consts::FRAC_2_PI;
    const FRAC_PI_2: f64 = std::f64::consts::FRAC_PI_2;

    let quadrant = (x * FRAC_2_PI).round();
    let r = x - quadrant * FRAC_PI_2;
    let r2 = r * r;
    let sin_r = r * (1.0 + r2 * (-1.0 / 6.0 + r2 * (1.0 / 120.0 + r2 * (-1.0 / 5040.0))));
    let cos_r =
        1.0 + r2 * (-1.0 / 2.0 + r2 * (1.0 / 24.0 + r2 * (-1.0 / 720.0 + r2 * (1.0 / 40320.0))));

    match quadrant as i64 & 3 {
        0 => (sin_r, cos_r),
        1 => (cos_r, -sin_r),
        2 => (-sin_r, -cos_r),
        _ => (-cos_r, sin_r),
    }
}

#[inline(always)]
fn next_point(params: Params, x: f64, y: f64) -> (f64, f64) {
    let (sin_ay, _) = fast_sin_cos(params.a * y);
    let (_, cos_ax) = fast_sin_cos(params.a * x);
    let (sin_bx, _) = fast_sin_cos(params.b * x);
    let (_, cos_by) = fast_sin_cos(params.b * y);
    (sin_ay + params.c * cos_ax, sin_bx + params.d * cos_by)
}

#[inline(always)]
fn point_to_pixel(mapper: Mapper, x: f64, y: f64) -> Option<usize> {
    let px = mapper.offset_x + x * mapper.scale_x;
    let py = mapper.offset_y - y * mapper.scale_y;
    let ix = px as isize;
    let iy = py as isize;

    if ix < 0 || ix >= mapper.width as isize || iy < 0 || iy >= mapper.height as isize {
        None
    } else {
        Some(iy as usize * mapper.width + ix as usize)
    }
}

fn seeded_point(worker: usize) -> (f64, f64) {
    let t = worker as f64 + 1.0;
    (
        0.1 + 0.01 * (12.9898 * t).sin(),
        0.1 + 0.01 * (78.233 * t).cos(),
    )
}

fn worker_iterations(total: u64, worker: usize, threads: usize) -> u64 {
    let base = total / threads as u64;
    let remainder = total % threads as u64;
    base + u64::from((worker as u64) < remainder)
}

fn iterate(config: &Config, params: Params) -> Vec<Count> {
    let pixels = config.width * config.height;
    let mut density = vec![0_u16; pixels];
    let mapper = Mapper::new(config);
    let completed = AtomicU64::new(0);
    let done = AtomicBool::new(false);

    thread::scope(|scope| {
        let monitor = config.progress.then(|| {
            scope.spawn(|| {
                let mut progress =
                    Progress::new("iterate", config.iterations, true).expect("progress is enabled");
                loop {
                    progress.set(completed.load(Ordering::Relaxed));
                    if done.load(Ordering::Acquire) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(250));
                }
                progress.finish();
            })
        });

        let mut handles = Vec::with_capacity(config.threads);
        for worker in 0..config.threads {
            let iterations = worker_iterations(config.iterations, worker, config.threads);
            let completed = &completed;
            handles.push(scope.spawn(move || {
                let mut local_density = vec![0_u16; pixels];
                let (mut x, mut y) = seeded_point(worker);
                let mut pending = 0_u64;

                for i in 1..=(config.warmup + iterations) {
                    let (next_x, next_y) = next_point(params, x, y);
                    x = next_x;
                    y = next_y;

                    if i > config.warmup {
                        if let Some(index) = point_to_pixel(mapper, x, y) {
                            local_density[index] = local_density[index].saturating_add(1);
                        }

                        pending += 1;
                        if pending == 1_000_000 {
                            completed.fetch_add(pending, Ordering::Relaxed);
                            pending = 0;
                        }
                    }
                }

                if pending > 0 {
                    completed.fetch_add(pending, Ordering::Relaxed);
                }

                local_density
            }));
        }

        for handle in handles {
            let local_density = handle.join().expect("render worker panicked");
            for (total, count) in density.iter_mut().zip(local_density) {
                *total = (*total).saturating_add(count);
            }
        }

        done.store(true, Ordering::Release);
        if let Some(monitor) = monitor {
            monitor.join().expect("progress monitor panicked");
        }
    });

    density
}

fn color_table(palette: Palette, scale: f64, max_seen: u32) -> Vec<[u8; 3]> {
    (0..=max_seen)
        .map(|count| {
            let t = if count == 0 {
                0.0
            } else {
                ((count as f64).ln_1p() / scale.ln_1p()).min(1.0).powf(0.92)
            };
            color_at(palette, t)
        })
        .collect()
}

fn ensure_parent(path: &PathBuf) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    Ok(())
}

fn default_exposure(config: &Config) -> f64 {
    let pixels = (config.width * config.height) as f64;
    let samples_per_pixel = config.iterations as f64 / pixels;
    (samples_per_pixel * 256.0).max(1.0)
}

fn write_ppm(config: &Config, density: &[Count]) -> io::Result<()> {
    let max_seen = density.iter().copied().map(u32::from).max().unwrap_or(0);
    let scale = config.exposure.unwrap_or_else(|| default_exposure(config));
    let palette = palette_by_name(&config.palette).map_err(io::Error::other)?;
    let colors = color_table(palette, scale, max_seen);

    ensure_parent(&config.out_path)?;
    let file = File::create(&config.out_path)?;
    let mut writer = BufWriter::new(file);
    write!(writer, "P6\n{} {}\n255\n", config.width, config.height)?;

    let mut row = vec![0_u8; config.width * 3];
    let mut progress = Progress::new("write", config.height as u64, config.progress);
    for y in 0..config.height {
        for x in 0..config.width {
            let color = colors[density[y * config.width + x] as usize];
            let offset = x * 3;
            row[offset] = color[0];
            row[offset + 1] = color[1];
            row[offset + 2] = color[2];
        }
        writer.write_all(&row)?;
        if y % 32 == 0 || y + 1 == config.height {
            if let Some(progress) = &mut progress {
                progress.set((y + 1) as u64);
            }
        }
    }

    if let Some(progress) = progress {
        progress.finish();
    }

    Ok(())
}

fn parse_value<T: std::str::FromStr>(name: &str, value: String) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value for --{name}: {value:?}"))
}

fn next_arg(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for --{name}"))
}

fn parse_config() -> Result<Config, String> {
    let default_threads = thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1);
    let mut config = Config {
        width: 15360,
        height: 8640,
        iterations: 1_000_000_000,
        warmup: 1_000,
        cutoff_x: 8.888888,
        cutoff_y: 5.0,
        exposure: None,
        palette: "aurora".to_string(),
        progress: true,
        threads: default_threads,
        out_path: PathBuf::from("out/16k.ppm"),
    };

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--width" => config.width = parse_value("width", next_arg(&mut args, "width")?)?,
            "--height" => config.height = parse_value("height", next_arg(&mut args, "height")?)?,
            "--iterations" => {
                config.iterations = parse_value("iterations", next_arg(&mut args, "iterations")?)?
            }
            "--warmup" => config.warmup = parse_value("warmup", next_arg(&mut args, "warmup")?)?,
            "--cutoff-x" => {
                config.cutoff_x = parse_value("cutoff-x", next_arg(&mut args, "cutoff-x")?)?
            }
            "--cutoff-y" => {
                config.cutoff_y = parse_value("cutoff-y", next_arg(&mut args, "cutoff-y")?)?
            }
            "--exposure" => {
                config.exposure = Some(parse_value("exposure", next_arg(&mut args, "exposure")?)?)
            }
            "--palette" => config.palette = next_arg(&mut args, "palette")?,
            "--threads" => {
                config.threads = parse_value("threads", next_arg(&mut args, "threads")?)?
            }
            "--quiet" => config.progress = false,
            "--out" => config.out_path = PathBuf::from(next_arg(&mut args, "out")?),
            "--help" | "-h" => {
                println!(
                    "Usage: attractors [--width N] [--height N] [--iterations N] [--palette dusk|ember|aurora] [--threads N] [--exposure N] [--quiet] [--out PATH]"
                );
                std::process::exit(0);
            }
            value if !value.starts_with('-') => config.out_path = PathBuf::from(value),
            _ => return Err(format!("unknown option {arg:?}")),
        }
    }

    if config.width == 0 {
        return Err("--width must be positive".to_string());
    }
    if config.height == 0 {
        return Err("--height must be positive".to_string());
    }
    if config.iterations == 0 {
        return Err("--iterations must be positive".to_string());
    }
    if config.threads == 0 {
        return Err("--threads must be positive".to_string());
    }
    palette_by_name(&config.palette)?;

    Ok(config)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = parse_config().map_err(io::Error::other)?;
    let density = iterate(&config, params());
    write_ppm(&config, &density)?;
    Ok(())
}
