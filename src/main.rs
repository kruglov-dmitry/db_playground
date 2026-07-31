use std::{
    fs::{File, create_dir_all},
    io::{BufWriter, Write},
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use postgres::{Client, NoTls};
use rand::{RngCore, rngs::ThreadRng};

const COPY_HEADER: &[u8] = b"PGCOPY\n\xff\r\n\0\0\0\0\0\0\0\0\0";
const ROW_BYTES: usize = 22; // int16 field count + int32 length + 16 UUID bytes

#[derive(Clone, Copy, Debug, ValueEnum)]
enum UuidKind {
    V4,
    V7,
}

#[derive(Parser, Debug)]
#[command(about = "Stream random UUIDs to PostgreSQL through binary COPY")]
struct Args {
    /// PostgreSQL connection string, e.g. postgres://benchmark:benchmark@localhost:54317/benchmark
    #[arg(long)]
    dsn: String,
    #[arg(long, value_enum)]
    uuid: UuidKind,
    /// Number of rows to insert (use 200000000 for the full benchmark)
    #[arg(long, default_value_t = 200_000_000)]
    rows: u64,
    /// Write this many inserted IDs to a CSV file for reproducible EXPLAIN runs
    #[arg(long, default_value_t = 1_000)]
    sample_size: usize,
    #[arg(long, default_value = "samples/ids.csv")]
    sample_output: String,
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

fn main() -> Result<()> {
    let args = Args::parse();
    if args.rows == 0 || args.batch_rows == 0 {
        bail!("--rows and --batch-rows must be positive");
    }
    let mut client = Client::connect(&args.dsn, NoTls).context("connecting to PostgreSQL")?;
    if args.truncate {
        client.batch_execute("TRUNCATE benchmark_items")?;
    }
    let mut copy = client.copy_in("COPY benchmark_items (id) FROM STDIN BINARY")?;
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
    let mut batch = Vec::with_capacity(args.batch_rows * ROW_BYTES);
    let started = Instant::now();
    let mut next_report = Instant::now() + Duration::from_secs(5);

    for n in 0..args.rows {
        let id = match args.uuid {
            UuidKind::V4 => uuid_v4(&mut rng),
            UuidKind::V7 => uuid_v7(&mut rng),
        };
        if (n as usize) < args.sample_size {
            writeln!(sample, "{}", uuid_text(&id))?;
        }
        batch.extend_from_slice(&1_i16.to_be_bytes());
        batch.extend_from_slice(&16_i32.to_be_bytes());
        batch.extend_from_slice(&id);
        if batch.len() >= args.batch_rows * ROW_BYTES {
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
    Ok(())
}
