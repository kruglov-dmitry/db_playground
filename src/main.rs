use std::{
    fs::{File, create_dir_all, rename},
    io::{BufWriter, Write},
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use postgres::{Client, NoTls};
use rand::{RngCore, rngs::ThreadRng};

const COPY_HEADER: &[u8] = b"PGCOPY\n\xff\r\n\0\0\0\0\0\0\0\0\0";
const UUID_ROW_BYTES: usize = 22; // int16 field count + int32 length + 16 UUID bytes
const BIGINT_ROW_BYTES: usize = 14; // int16 field count + int32 length + int64 bytes

#[derive(Clone, Copy, Debug, ValueEnum)]
enum KeyType {
    Uuid,
    Bigint,
}

impl KeyType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Uuid => "uuid",
            Self::Bigint => "bigint",
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum UuidKind {
    V4,
    V7,
}

impl UuidKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::V4 => "v4",
            Self::V7 => "v7",
        }
    }
}

#[derive(Parser, Debug)]
#[command(about = "Stream UUID or BIGINT primary keys to PostgreSQL through binary COPY")]
struct Args {
    /// PostgreSQL connection string, e.g. postgres://benchmark:benchmark@localhost:54317/benchmark
    #[arg(long)]
    dsn: String,
    /// Key representation to load
    #[arg(long, value_enum, default_value_t = KeyType::Uuid)]
    key_type: KeyType,
    /// UUID version; required when --key-type uuid
    #[arg(long, value_enum)]
    uuid: Option<UuidKind>,
    /// Destination table (must be a simple lowercase SQL identifier)
    #[arg(long, default_value = "benchmark_items")]
    table: String,
    /// Number of rows to insert (use 200000000 for the full benchmark)
    #[arg(long, default_value_t = 200_000_000)]
    rows: u64,
    /// Write this many inserted IDs to a CSV file for reproducible EXPLAIN runs
    #[arg(long, default_value_t = 1_000)]
    sample_size: usize,
    #[arg(long, default_value = "samples/ids.csv")]
    sample_output: String,
    /// Label used for the saved load-time result (defaults to the sample file name)
    #[arg(long)]
    label: Option<String>,
    /// Directory where the completed load result is written as <label>.load.json
    #[arg(long, default_value = "results")]
    load_results_dir: String,
    /// Delete existing rows before loading
    #[arg(long)]
    truncate: bool,
    /// Rows accumulated per write to the COPY stream
    #[arg(long, default_value_t = 65_536)]
    batch_rows: usize,
}

fn uuid_v4(rng: &mut ThreadRng) -> [u8; 16] {
    let mut id = [0_u8; 16];
    rng.fill_bytes(&mut id);
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    id
}

// RFC 9562 UUIDv7: a millisecond Unix timestamp followed by random bits.
fn uuid_v7(rng: &mut ThreadRng) -> [u8; 16] {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let mut id = [0_u8; 16];
    id[..6].copy_from_slice(&millis.to_be_bytes()[2..]);
    rng.fill_bytes(&mut id[6..]);
    id[6] = (id[6] & 0x0f) | 0x70;
    id[8] = (id[8] & 0x3f) | 0x80;
    id
}

fn uuid_text(id: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        id[0],
        id[1],
        id[2],
        id[3],
        id[4],
        id[5],
        id[6],
        id[7],
        id[8],
        id[9],
        id[10],
        id[11],
        id[12],
        id[13],
        id[14],
        id[15]
    )
}

fn is_simple_identifier(name: &str) -> bool {
    let mut chars = name.bytes();
    matches!(chars.next(), Some(b'a'..=b'z' | b'_'))
        && chars.all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

fn is_safe_label(label: &str) -> bool {
    !label.is_empty()
        && label
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'))
}

fn label_from_sample(sample_output: &str) -> Result<String> {
    Path::new(sample_output)
        .file_stem()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .context("deriving a label from --sample-output; pass --label explicitly")
}

fn write_load_result(args: &Args, label: &str, seconds: f64, uuid: Option<UuidKind>) -> Result<()> {
    let output_dir = Path::new(&args.load_results_dir);
    create_dir_all(output_dir).context("creating load results directory")?;
    let output = output_dir.join(format!("{label}.load.json"));
    let temporary = output_dir.join(format!(".{label}.load.json.tmp"));
    let rows_per_second = args.rows as f64 / seconds;
    let json = format!(
        concat!(
            "{{\n",
            "  \"label\": \"{label}\",\n",
            "  \"table\": \"{table}\",\n",
            "  \"key_type\": \"{key_type}\",\n",
            "  \"uuid_version\": {uuid_version},\n",
            "  \"rows\": {rows},\n",
            "  \"duration_seconds\": {seconds:.3},\n",
            "  \"rows_per_second\": {rows_per_second:.0}\n",
            "}}\n"
        ),
        label = label,
        table = args.table,
        key_type = args.key_type.as_str(),
        uuid_version = uuid
            .map(|kind| format!("\"{}\"", kind.as_str()))
            .unwrap_or_else(|| "null".to_owned()),
        rows = args.rows,
        seconds = seconds,
        rows_per_second = rows_per_second,
    );
    std::fs::write(&temporary, json).context("writing temporary load result")?;
    rename(&temporary, &output).context("saving load result")?;
    eprintln!("saved load result: {}", output.display());
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.rows == 0 || args.batch_rows == 0 {
        bail!("--rows and --batch-rows must be positive");
    }
    if !is_simple_identifier(&args.table) {
        bail!("--table must be a simple lowercase SQL identifier");
    }
    let uuid = match args.key_type {
        KeyType::Uuid => Some(
            args.uuid
                .context("--uuid is required when --key-type uuid")?,
        ),
        KeyType::Bigint => {
            if args.uuid.is_some() {
                bail!("--uuid cannot be used when --key-type bigint");
            }
            None
        }
    };
    let label = args
        .label
        .clone()
        .map(Ok)
        .unwrap_or_else(|| label_from_sample(&args.sample_output))?;
    if !is_safe_label(&label) {
        bail!("--label must contain only lowercase letters, digits, hyphens, or underscores");
    }
    let mut client = Client::connect(&args.dsn, NoTls).context("connecting to PostgreSQL")?;
    if args.truncate {
        client.batch_execute(&format!("TRUNCATE {}", args.table))?;
    }
    let mut copy = client.copy_in(&format!("COPY {} (id) FROM STDIN BINARY", args.table))?;
    copy.write_all(COPY_HEADER)?;
    if let Some(parent) = Path::new(&args.sample_output)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        create_dir_all(parent).context("creating sample output directory")?;
    }
    let mut sample =
        BufWriter::new(File::create(&args.sample_output).context("creating sample output")?);
    let mut rng = rand::rng();
    let row_bytes = match args.key_type {
        KeyType::Uuid => UUID_ROW_BYTES,
        KeyType::Bigint => BIGINT_ROW_BYTES,
    };
    let mut batch = Vec::with_capacity(args.batch_rows * row_bytes);
    let started = Instant::now();
    let mut next_report = Instant::now() + Duration::from_secs(5);

    for n in 0..args.rows {
        batch.extend_from_slice(&1_i16.to_be_bytes());
        match args.key_type {
            KeyType::Uuid => {
                let id = match uuid.expect("UUID type requires a UUID version") {
                    UuidKind::V4 => uuid_v4(&mut rng),
                    UuidKind::V7 => uuid_v7(&mut rng),
                };
                if (n as usize) < args.sample_size {
                    writeln!(sample, "{}", uuid_text(&id))?;
                }
                batch.extend_from_slice(&16_i32.to_be_bytes());
                batch.extend_from_slice(&id);
            }
            KeyType::Bigint => {
                let id = i64::try_from(n + 1).context("BIGINT key overflow")?;
                if (n as usize) < args.sample_size {
                    writeln!(sample, "{id}")?;
                }
                batch.extend_from_slice(&8_i32.to_be_bytes());
                batch.extend_from_slice(&id.to_be_bytes());
            }
        }
        if batch.len() >= args.batch_rows * row_bytes {
            copy.write_all(&batch)?;
            batch.clear();
        }
        if Instant::now() >= next_report {
            let elapsed = started.elapsed().as_secs_f64();
            eprintln!("{n} rows sent ({:.0} rows/s)", n as f64 / elapsed);
            next_report += Duration::from_secs(5);
        }
    }
    if !batch.is_empty() {
        copy.write_all(&batch)?;
    }
    copy.write_all(&(-1_i16).to_be_bytes())?;
    copy.finish()?;
    sample.flush()?;
    let seconds = started.elapsed().as_secs_f64();
    eprintln!(
        "finished {} rows in {:.1}s ({:.0} rows/s)",
        args.rows,
        seconds,
        args.rows as f64 / seconds
    );
    write_load_result(&args, &label, seconds, uuid)?;
    Ok(())
}
