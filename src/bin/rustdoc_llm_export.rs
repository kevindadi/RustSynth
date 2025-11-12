use std::{
    env,
    fs::File,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{bail, Context, Result};
use trustfall_rustdoc_adapter::{
    export::{build_llm_spec, ExportOptions},
    Crate, IndexedCrate,
};

fn main() -> Result<()> {
    let args = parse_args()?;
    let file = File::open(&args.rustdoc_json)
        .with_context(|| format!("unable to open rustdoc JSON at {}", args.rustdoc_json.display()))?;
    let crate_data: Crate = serde_json::from_reader(file).context("failed to deserialize rustdoc JSON")?;
    let indexed = IndexedCrate::new(&crate_data);

    let mut options = ExportOptions::default();
    options.public_only = args.public_only;
    options.max_doc_bytes = args.max_doc_bytes;
    options.skip_panic_pass = args.skip_panic_pass;
    options.crate_name_override = args.crate_name;

    let spec = build_llm_spec(&indexed, &options).context("failed to build LLM specification")?;

    match args.output {
        Some(path) => {
            let file = File::create(&path)
                .with_context(|| format!("failed to create output file {}", path.display()))?;
            if args.pretty {
                serde_json::to_writer_pretty(file, &spec).context("failed to write pretty JSON")?;
            } else {
                serde_json::to_writer(file, &spec).context("failed to write JSON")?;
            }
        }
        None => {
            let stdout = io::stdout();
            let handle = stdout.lock();
            if args.pretty {
                serde_json::to_writer_pretty(handle, &spec).context("failed to write pretty JSON")?;
            } else {
                serde_json::to_writer(handle, &spec).context("failed to write JSON")?;
            }
        }
    }

    Ok(())
}

struct CliArgs {
    rustdoc_json: PathBuf,
    output: Option<PathBuf>,
    crate_name: Option<String>,
    public_only: bool,
    pretty: bool,
    max_doc_bytes: Option<usize>,
    skip_panic_pass: bool,
}

fn parse_args() -> Result<CliArgs> {
    let mut args = env::args().skip(1);
    let mut config = CliArgs {
        rustdoc_json: PathBuf::new(),
        output: None,
        crate_name: None,
        public_only: true,
        pretty: false,
        max_doc_bytes: Some(32 * 1024),
        skip_panic_pass: false,
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            "--rustdoc-json" => {
                let value = args.next().ok_or_else(|| anyhow::anyhow!("--rustdoc-json requires a value"))?;
                config.rustdoc_json = PathBuf::from(value);
            }
            "--output" | "-o" => {
                let value = args.next().ok_or_else(|| anyhow::anyhow!("--output requires a value"))?;
                config.output = Some(PathBuf::from(value));
            }
            "--crate-name" => {
                let value = args.next().ok_or_else(|| anyhow::anyhow!("--crate-name requires a value"))?;
                config.crate_name = Some(value);
            }
            "--public-only" => {
                config.public_only = true;
            }
            "--include-private" => {
                config.public_only = false;
            }
            "--pretty" => {
                config.pretty = true;
            }
            "--max-doc-bytes" => {
                let value = args.next().ok_or_else(|| anyhow::anyhow!("--max-doc-bytes requires a value"))?;
                config.max_doc_bytes = parse_doc_limit(&value)?;
            }
            "--no-panics-pass" => {
                config.skip_panic_pass = true;
            }
            other => {
                bail!("unrecognized argument: {other}");
            }
        }
    }

    if config.rustdoc_json.as_os_str().is_empty() {
        bail!("missing required argument: --rustdoc-json <path>");
    }

    Ok(config)
}

fn parse_doc_limit(value: &str) -> Result<Option<usize>> {
    if value.eq_ignore_ascii_case("none") || value.eq_ignore_ascii_case("unlimited") {
        return Ok(None);
    }
    let limit: usize = value
        .parse()
        .with_context(|| format!("invalid value for --max-doc-bytes: {value}"))?;
    if limit == 0 {
        Ok(None)
    } else {
        Ok(Some(limit))
    }
}

fn print_usage() {
    const USAGE: &str = r#"Usage: rustdoc_llm_export --rustdoc-json <path> [options]

Options:
    --rustdoc-json <path>     Path to rustdoc JSON input (required)
    -o, --output <path>       Output file (defaults to stdout)
        --crate-name <name>   Override crate name in exported spec
        --public-only         Export only public items (default)
        --include-private     Export all items, including private ones
        --pretty              Pretty-print JSON output
        --max-doc-bytes <n>   Truncate docs to N bytes (use 0/none for unlimited)
        --no-panics-pass      Skip panic/error heuristic analysis
    -h, --help                Show this help message
"#;
    let _ = writeln!(io::stderr(), "{USAGE}");
}

