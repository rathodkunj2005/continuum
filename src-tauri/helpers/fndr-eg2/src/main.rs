//! `fndr-eg2-helper`: FNDR's text embedding engine, EmbeddingGemma 2 on a
//! llama.cpp of its own, in a process of its own so the model never shares
//! memory or a llama.cpp build with Qwen and a Metal fault cannot take the
//! app down.
//!
//! Serve:  fndr-eg2-helper --text <text.gguf> [--cpu]
//!   Prints one ready line, then answers one JSON request per stdin line with
//!   one JSON line on stdout. Quits when stdin closes (the app went away).
//!   {"id":1,"op":"embed_texts","texts":["task: search result | query: ..."]}
//!
//! Check:  fndr-eg2-helper --check <reference dir> --text <text.gguf>
//!   Embeds DigUp's reference texts and exits 0 only if every cosine is at
//!   least 0.999 (the bar DigUp holds each llama.cpp update to).

mod gemma;

#[allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    dead_code,
    clippy::all
)]
mod sys {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

use gemma::GemmaEmbedder;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const AGREEMENT: f32 = 0.999;

#[derive(Default)]
struct Args {
    text: Option<PathBuf>,
    cpu: bool,
    check: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut raw = std::env::args().skip(1);
    while let Some(flag) = raw.next() {
        match flag.as_str() {
            "--text" => args.text = raw.next().map(PathBuf::from),
            "--check" => args.check = raw.next().map(PathBuf::from),
            "--cpu" => args.cpu = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

fn main() {
    quiet_llama_logs();
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let Some(text) = args.text.clone() else {
        eprintln!("usage: fndr-eg2-helper --text <text.gguf> [--cpu] [--check <dir>]");
        std::process::exit(2);
    };
    let started = Instant::now();
    let mut embedder = match GemmaEmbedder::load(&text, args.cpu) {
        Ok(embedder) => embedder,
        Err(error) => {
            println!("{}", json!({ "ready": false, "error": error }));
            std::process::exit(1);
        }
    };
    let ready = json!({
        "ready": true,
        "dimension": embedder.dimension(),
        "vector_version": gemma::VECTOR_VERSION,
        "llama": env!("FNDR_LLAMA_BUILD"),
        "load_seconds": (started.elapsed().as_secs_f64() * 100.0).round() / 100.0,
    });
    let code = match args.check {
        Some(dir) => {
            eprintln!("{ready}");
            check(&mut embedder, &dir)
        }
        None => {
            println!("{ready}");
            serve(&mut embedder);
            0
        }
    };
    // The model and the Metal backend go before exit: ggml's static Metal device
    // asserts at exit if a model still holds its buffers.
    drop(embedder);
    unsafe { sys::llama_backend_free() };
    std::process::exit(code);
}

fn serve(embedder: &mut GemmaEmbedder) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(request) => answer(embedder, &request),
            Err(error) => json!({ "ok": false, "error": format!("bad request: {error}") }),
        };
        if writeln!(stdout, "{reply}")
            .and_then(|_| stdout.flush())
            .is_err()
        {
            break;
        }
    }
}

fn answer(embedder: &mut GemmaEmbedder, request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let result = match request.get("op").and_then(Value::as_str) {
        Some("embed_texts") => {
            let texts: Vec<String> = request
                .get("texts")
                .and_then(Value::as_array)
                .map(|texts| {
                    texts
                        .iter()
                        .filter_map(|t| t.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            embedder.embed_texts(&texts)
        }
        other => Err(format!("unknown op {other:?}")),
    };
    match result {
        Ok(vectors) => json!({ "id": id, "ok": true, "vectors": vectors }),
        Err(error) => json!({ "id": id, "ok": false, "error": error }),
    }
}

#[derive(Deserialize)]
struct Reference {
    made_with: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    input: String,
    vector: Vec<f32>,
}

/// DigUp's reference vectors for its text inputs, alone and then all in one
/// `llama_process` call.
fn check(embedder: &mut GemmaEmbedder, dir: &Path) -> i32 {
    let reference: Reference = match std::fs::read_to_string(dir.join("reference.json"))
        .map_err(|error| error.to_string())
        .and_then(|text| serde_json::from_str(&text).map_err(|error| error.to_string()))
    {
        Ok(reference) => reference,
        Err(error) => {
            eprintln!(
                "can't read {}: {error}",
                dir.join("reference.json").display()
            );
            return 2;
        }
    };
    let (mut ran, mut failures) = (0, 0);
    let mut checked: Vec<&Case> = Vec::new();
    for case in &reference.cases {
        if !embedder.fits(&case.input) {
            println!("{:<14} skipped: longer than this context", case.name);
            continue;
        }
        let timer = Instant::now();
        let made = embedder
            .embed_texts(std::slice::from_ref(&case.input))
            .map(|mut v| v.remove(0));
        ran += 1;
        match made {
            Ok(vector) => {
                let cosine = dot(&vector, &case.vector);
                let ok = vector.len() == case.vector.len() && cosine >= AGREEMENT;
                failures += usize::from(!ok);
                checked.push(case);
                println!(
                    "{:<14} {:>8.1} ms  cosine {:.5}  {}",
                    case.name,
                    timer.elapsed().as_secs_f64() * 1000.0,
                    cosine,
                    if ok { "ok" } else { "FAIL" }
                );
            }
            Err(error) => {
                failures += 1;
                println!("{:<14} ERROR {error}", case.name);
            }
        }
    }
    let inputs: Vec<String> = checked.iter().map(|case| case.input.clone()).collect();
    let worst = match embedder.embed_texts(&inputs) {
        Ok(vectors) => checked
            .iter()
            .zip(&vectors)
            .map(|(case, v)| dot(v, &case.vector))
            .fold(1.0, f32::min),
        Err(_) => 0.0,
    };
    ran += 1;
    failures += usize::from(worst < AGREEMENT);
    println!(
        "batched texts  worst cosine {worst:.5}  {}",
        if worst >= AGREEMENT { "ok" } else { "FAIL" }
    );
    println!(
        "reference made with {}; this is llama.cpp {}; {} of {} agree",
        reference.made_with,
        env!("FNDR_LLAMA_BUILD"),
        ran - failures,
        ran
    );
    i32::from(failures > 0)
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// llama.cpp talks a lot on stderr; keep errors only.
fn quiet_llama_logs() {
    unsafe extern "C" fn log(
        level: sys::ggml_log_level,
        text: *const std::os::raw::c_char,
        _: *mut std::ffi::c_void,
    ) {
        if level == sys::GGML_LOG_LEVEL_ERROR && !text.is_null() {
            eprint!(
                "{}",
                unsafe { std::ffi::CStr::from_ptr(text) }.to_string_lossy()
            );
        }
    }
    unsafe { sys::llama_log_set(Some(log), std::ptr::null_mut()) };
}
