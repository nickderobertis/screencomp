//! Criterion micro-benchmarks for the in-process work of each `screencomp` verb.
//!
//! These measure everything a single invocation does after process start: walk
//! the `<root>/<project>/<name>.png` tree, SHA-256 every file, classify the two
//! snapshots against each other, and render the requested output. Process
//! startup and terminal I/O are deliberately excluded here — `scripts/bench.sh`
//! covers those end to end with hyperfine.
//!
//! The hot path is the directory walk plus a byte digest over every file
//! (`io::fs::discover`), so total bytes (projects × shots × size) is what these
//! numbers move; the pure comparison and rendering on top of it is cheap. To
//! keep the digest cost honest the synthetic trees are built once on disk,
//! outside every timed loop, and each verb is benched at two scales so the
//! slope with image count is visible. The sets are intentionally modest — a
//! floor, not a worst case.
//!
//! Everything runs through the crate's only public entrypoint, [`run`], so the
//! numbers track what the binary runs rather than internals that may be inlined
//! away. The arg tree is constructed directly (not parsed) to keep clap out of
//! the measurement.

// `criterion_group!`/`criterion_main!` expand to undocumented public functions;
// a bench harness is not a documented API surface, so the crate-wide
// `missing_docs = "deny"` is relaxed for this target only.
#![allow(missing_docs)]

use std::fs;
use std::hint::black_box;
use std::io;

use camino::{Utf8Path, Utf8PathBuf};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use screencomp::cli::{ClassifyArgs, Command, CommentArgs, GalleryArgs, OutputFormat};
use screencomp::{Cli, run};
use tempfile::TempDir;

/// Shape of one synthetic screenshot tree: `projects` directories each holding
/// `shots` PNGs of `kib` kibibytes.
struct TreeSpec {
    name: &'static str,
    projects: usize,
    shots: usize,
    kib: usize,
}

/// The scales every verb is benched at. `small` is a typical per-PR capture;
/// `large` shows how cost scales with image count and size.
fn specs() -> Vec<TreeSpec> {
    vec![
        TreeSpec {
            name: "small",
            projects: 4,
            shots: 6,
            kib: 8,
        },
        TreeSpec {
            name: "large",
            projects: 12,
            shots: 12,
            kib: 16,
        },
    ]
}

/// A built pair of trees on disk, kept alive by its owning [`TempDir`].
struct Trees {
    name: &'static str,
    _dir: TempDir,
    baseline: Utf8PathBuf,
    current: Utf8PathBuf,
    gallery_out: Utf8PathBuf,
}

/// Deterministic, distinct-per-`seed` bytes — no image codec is involved because
/// the tool compares files by byte digest, so any reproducible content of the
/// right length exercises the same path a real capture would.
fn write_shot(path: &Utf8Path, seed: u64, kib: usize) -> io::Result<()> {
    let len = kib * 1024;
    let mut buf = Vec::with_capacity(len);
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    while buf.len() < len {
        // xorshift64: cheap, deterministic, and avoids long runs of identical
        // bytes so the digest does real work.
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        buf.extend_from_slice(&state.to_le_bytes());
    }
    buf.truncate(len);
    fs::write(path, &buf)
}

/// Build a baseline/current pair under a fresh tempdir. `current` mirrors
/// `baseline` except for a realistic mix: ~1 shot per project changed, one added
/// (only in current), and one removed (only in baseline).
fn build(spec: &TreeSpec) -> Trees {
    let dir = TempDir::new().expect("tempdir");
    let root = Utf8Path::from_path(dir.path()).expect("utf-8 tempdir path");
    let baseline = root.join("baseline");
    let current = root.join("current");
    let gallery_out = root.join("gallery-out");

    for p in 0..spec.projects {
        let proj = format!("project{p:02}");
        let bdir = baseline.join(&proj);
        let cdir = current.join(&proj);
        fs::create_dir_all(&bdir).expect("create baseline project");
        fs::create_dir_all(&cdir).expect("create current project");
        for s in 0..spec.shots {
            let name = format!("shot{s:02}.png");
            let seed = (p * spec.shots + s) as u64;
            write_shot(&bdir.join(&name), seed, spec.kib).expect("write baseline shot");
            let cseed = if s % 8 == 0 { seed ^ 0xFFFF } else { seed };
            write_shot(&cdir.join(&name), cseed, spec.kib).expect("write current shot");
        }
        write_shot(&cdir.join("added.png"), 0xADDE_0000 + p as u64, spec.kib).expect("write added");
        write_shot(&bdir.join("removed.png"), 0xDEAD_0000 + p as u64, spec.kib)
            .expect("write removed");
    }

    Trees {
        name: spec.name,
        _dir: dir,
        baseline,
        current,
        gallery_out,
    }
}

/// Build the tree pair for every spec once, ahead of any timed loop.
fn all_trees() -> Vec<Trees> {
    specs().iter().map(build).collect()
}

fn classify_cli(t: &Trees, format: OutputFormat) -> Cli {
    Cli {
        quiet: false,
        config: None,
        command: Command::Classify(ClassifyArgs {
            baseline: Some(t.baseline.clone()),
            baseline_manifest: None,
            current: t.current.clone(),
            arch: None,
            format,
            exit_code: false,
        }),
    }
}

fn gallery_cli(t: &Trees) -> Cli {
    Cli {
        quiet: true,
        config: None,
        command: Command::Gallery(GalleryArgs {
            input: t.current.clone(),
            baseline: Some(t.baseline.clone()),
            arch: None,
            output: t.gallery_out.clone(),
            title: "Screenshot gallery".to_owned(),
        }),
    }
}

fn comment_cli(t: &Trees) -> Cli {
    Cli {
        quiet: true,
        config: None,
        command: Command::Comment(CommentArgs {
            baseline: Some(t.baseline.clone()),
            baseline_manifest: None,
            current: t.current.clone(),
            arch: None,
            title: None,
            marker: None,
            gallery_url: None,
            baseline_url: None,
            current_url: None,
            embed_limit: None,
            output: None,
        }),
    }
}

/// Drive each verb over a fresh `Cli` (cheap path clones), writing user output to
/// a reused sink so allocation noise stays out of the measurement.
fn bench_verb(c: &mut Criterion, group_name: &str, make: impl Fn(&Trees) -> Cli) {
    let trees = all_trees();
    let mut group = c.benchmark_group(group_name);
    for t in &trees {
        group.bench_with_input(BenchmarkId::from_parameter(t.name), t, |b, t| {
            let mut sink = Vec::new();
            b.iter(|| {
                sink.clear();
                run(black_box(make(t)), black_box(&mut sink)).expect("run");
                black_box(&sink);
            });
        });
    }
    group.finish();
}

fn bench_classify(c: &mut Criterion) {
    bench_verb(c, "classify", |t| classify_cli(t, OutputFormat::Human));
}

fn bench_classify_json(c: &mut Criterion) {
    bench_verb(c, "classify_json", |t| classify_cli(t, OutputFormat::Json));
}

fn bench_gallery(c: &mut Criterion) {
    bench_verb(c, "gallery", gallery_cli);
}

fn bench_comment(c: &mut Criterion) {
    bench_verb(c, "comment", comment_cli);
}

criterion_group!(
    benches,
    bench_classify,
    bench_classify_json,
    bench_gallery,
    bench_comment
);
criterion_main!(benches);
