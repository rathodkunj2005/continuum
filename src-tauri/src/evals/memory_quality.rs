//! LLM-as-judge eval harness for memory synthesis quality.
//!
//! Runs synthetic captures through the deterministic part of the pipeline
//! (`insight_from_ocr_only` + `derive_insight_for_record`) and asks the
//! local LLM to score each output for informativeness. This is the
//! domain-agnostic alternative to hardcoded assertions: as long as the
//! prompt asks "is this informative?", improvements to synthesis quality
//! will show as higher scores across all categories.
//!
//! Results are appended to `target/memory_quality_evals.jsonl` for diffing.
//!
//! Run with: `cargo test --lib eval_memory_quality -- --ignored --nocapture`

use crate::inference::InferenceEngine;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct SyntheticCapture {
    pub id: String,
    pub app_name: String,
    pub window_title: String,
    pub ocr_text: String,
    /// Hint to the judge so it can score domain-fit, never used as
    /// a hardcoded assertion.
    #[serde(default)]
    pub domain_hint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalResult {
    pub capture_id: String,
    pub app_name: String,
    pub window_title: String,
    pub insight_what_happened: String,
    pub insight_why_mattered: String,
    pub topic_categories: Vec<String>,
    pub search_aliases: Vec<String>,
    pub judge_score: f32,
    pub judge_reason: String,
    pub has_filename_pattern: bool,
    pub has_template_text: bool,
    pub mentions_app_redundantly: bool,
    pub what_happened_word_count: usize,
}

/// Tag heuristics — used to flag obvious failures without hardcoding
/// domain-specific terms. These mirror the guards in `derive_insight`.
fn has_filename_pattern(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    let digit_count = lower.chars().filter(|c| c.is_ascii_digit()).count();
    lower.contains(".png") && digit_count >= 6
}

fn has_template_text(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.contains("screen capture (visual)")
        || lower.contains("captured recent activity")
        || lower.contains("url-only surface capture")
}

/// Detect when the app name appears multiple times in the insight (fluff).
fn mentions_app_redundantly(insight: &str, app_name: &str) -> bool {
    if app_name.trim().is_empty() {
        return false;
    }
    let lower_insight = insight.to_ascii_lowercase();
    let lower_app = app_name.to_ascii_lowercase();
    lower_insight.matches(&lower_app).count() >= 2
}

/// Use the LLM to score an insight on a 0-1 scale.
/// Returns (score, reason). The judge prompt is intentionally generic —
/// no domain-specific criteria, just informativeness signals.
pub async fn judge_insight_quality(
    engine: &InferenceEngine,
    app_name: &str,
    window_title: &str,
    what_happened: &str,
    why_mattered: &str,
    topic_categories: &[String],
) -> (f32, String) {
    let prompt_body = format!(
        "Evaluate this captured screen memory. App: \"{app_name}\". Window: \"{window_title}\".\n\
         what_happened: \"{what}\"\n\
         why_mattered:  \"{why}\"\n\
         topic_categories: [{cats}]\n\n\
         Rate 0.0-1.0:\n\
         1.0 = Specific, names the task/event/objective (e.g., \"You reviewed PR #42 for auth\")\n\
         0.7 = Identifiable but generic (\"You used the IDE\")\n\
         0.4 = Mostly metadata, low information value\n\
         0.0 = Template/filename/empty, no real content\n\n\
         Penalize: redundant app/website name repetition, fluff (\"the user did things\"), \
         template phrases (\"Screen capture\", \"Captured recent\"), filenames with timestamps.\n\
         Reward: specific nouns, named artifacts, action verbs, semantic categories.\n\n\
         Respond exactly:  score: X.X | reason: <one short sentence>",
        what = what_happened,
        why = why_mattered,
        cats = topic_categories.join(", "),
    );

    let raw = engine.answer(&prompt_body, "").await;
    parse_judge_output(&raw)
}

fn parse_judge_output(raw: &str) -> (f32, String) {
    let lower = raw.to_ascii_lowercase();
    if let Some(pos) = lower.find("score:") {
        let after = &raw[pos + 6..];
        let pipe = after.find('|').unwrap_or(after.len());
        let score_str = after[..pipe].trim();
        let score = score_str
            .trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
            .parse::<f32>()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);

        let reason = if pipe < after.len() {
            let tail = &after[pipe + 1..];
            // Drop optional "reason:" prefix
            let tail_lower = tail.to_ascii_lowercase();
            if let Some(rpos) = tail_lower.find("reason:") {
                tail[rpos + 7..].trim().to_string()
            } else {
                tail.trim().to_string()
            }
        } else {
            String::new()
        };
        return (score, reason);
    }
    (
        0.0,
        format!(
            "could not parse judge output: {}",
            &raw[..raw.len().min(120)]
        ),
    )
}

/// Run one synthetic capture through the pipeline and judge it.
/// Pure function — does not touch storage or capture loops.
pub async fn evaluate_capture(
    engine: &InferenceEngine,
    capture: &SyntheticCapture,
) -> EvalResult {
    use crate::inference::insight_from_ocr_only;
    use crate::memory_insight::derive_insight_for_record;
    use crate::storage::MemoryRecord;

    // 1) Synth: deterministic insight extraction from OCR
    let insight = insight_from_ocr_only(
        "synthetic_eval.png",
        Some(&capture.app_name),
        Some(&capture.window_title),
        &capture.ocr_text,
    );

    // 2) Build a minimal MemoryRecord — only fields used by derive_insight
    let mut record = MemoryRecord {
        id: format!("eval-{}", capture.id),
        app_name: capture.app_name.clone(),
        window_title: capture.window_title.clone(),
        clean_text: capture.ocr_text.clone(),
        ocr_confidence: 0.85,
        topic: insight
            .topics
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        topic_categories: insight
            .topics
            .iter()
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty() && t.len() <= 40)
            .collect(),
        search_aliases: insight.search_aliases.clone(),
        entities: insight.entities.clone(),
        activity_type: insight.activity_type.clone().unwrap_or_default(),
        user_intent: insight.user_intent.clone().unwrap_or_default(),
        display_summary: insight.summary_short.clone(),
        memory_context: insight.summary_detailed.clone(),
        insight_what_happened: String::new(), // force derivation
        insight_why_mattered: String::new(),
        ..MemoryRecord::default()
    };

    derive_insight_for_record(&mut record);

    let (score, reason) = judge_insight_quality(
        engine,
        &capture.app_name,
        &capture.window_title,
        &record.insight_what_happened,
        &record.insight_why_mattered,
        &record.topic_categories,
    )
    .await;

    EvalResult {
        capture_id: capture.id.clone(),
        app_name: capture.app_name.clone(),
        window_title: capture.window_title.clone(),
        insight_what_happened: record.insight_what_happened.clone(),
        insight_why_mattered: record.insight_why_mattered.clone(),
        topic_categories: record.topic_categories.clone(),
        search_aliases: record.search_aliases.clone(),
        judge_score: score,
        judge_reason: reason,
        has_filename_pattern: has_filename_pattern(&record.insight_what_happened),
        has_template_text: has_template_text(&record.insight_what_happened),
        mentions_app_redundantly: mentions_app_redundantly(
            &record.insight_what_happened,
            &capture.app_name,
        ),
        what_happened_word_count: record
            .insight_what_happened
            .split_whitespace()
            .count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_judge_output_extracts_score_and_reason() {
        let (s, r) = parse_judge_output("score: 0.85 | reason: specific named artifact and clear outcome.");
        assert!((s - 0.85).abs() < 1e-3, "got {s}");
        assert!(r.contains("specific"), "got: {r}");
    }

    #[test]
    fn parse_judge_output_handles_missing_reason_prefix() {
        let (s, r) = parse_judge_output("score: 0.5 | low specificity");
        assert!((s - 0.5).abs() < 1e-3);
        assert!(r.contains("low"));
    }

    #[test]
    fn parse_judge_output_clamps_invalid_scores() {
        let (s, _) = parse_judge_output("score: 9.9 | reason: junk");
        assert_eq!(s, 1.0);
        let (s, _) = parse_judge_output("score: -1 | reason: junk");
        assert_eq!(s, 0.0);
    }

    #[test]
    fn parse_judge_output_handles_garbage() {
        let (s, r) = parse_judge_output("blah blah no score here");
        assert_eq!(s, 0.0);
        assert!(r.starts_with("could not parse"));
    }

    #[test]
    fn has_filename_pattern_detects_timestamped_png() {
        assert!(has_filename_pattern("Screen capture (visual): Claude_1778938598807.png"));
        assert!(!has_filename_pattern("You reviewed the PR"));
    }

    #[test]
    fn mentions_app_redundantly_detects_double_app_name() {
        assert!(mentions_app_redundantly(
            "Google Chrome — Google Chrome news article",
            "Google Chrome"
        ));
        assert!(!mentions_app_redundantly(
            "You reviewed a news article",
            "Google Chrome"
        ));
    }

    #[test]
    #[ignore = "requires local LLM model — run with: cargo test eval_memory_quality_across_domains -- --ignored --nocapture"]
    fn eval_memory_quality_across_domains() {
        // Run with tokio runtime since judge_insight_quality is async
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        runtime.block_on(async {
            let fixtures_path = std::env::current_dir()
                .unwrap()
                .join("tests/fixtures/synthetic_captures/captures.json");
            let raw = std::fs::read_to_string(&fixtures_path).unwrap_or_else(|e| {
                panic!("fixture file missing at {:?}: {e}", fixtures_path)
            });
            let captures: Vec<SyntheticCapture> =
                serde_json::from_str(&raw).expect("fixture is valid JSON");

            let engine = InferenceEngine::new(None, None)
                .await
                .expect("InferenceEngine init — is a local model downloaded?");

            let mut total = 0.0f32;
            let mut results = Vec::with_capacity(captures.len());
            let mut hard_failures = Vec::new();

            for cap in &captures {
                let res = evaluate_capture(&engine, cap).await;
                eprintln!(
                    "[{}] score={:.2}  what={}",
                    res.capture_id,
                    res.judge_score,
                    &res.insight_what_happened[..res.insight_what_happened.len().min(90)],
                );
                if res.has_filename_pattern || res.has_template_text {
                    hard_failures.push(res.capture_id.clone());
                }
                total += res.judge_score;
                results.push(res);
            }

            // Append run results for offline inspection.
            let out_path = std::env::current_dir()
                .unwrap()
                .join("target/memory_quality_evals.jsonl");
            if let Ok(mut f) = std::fs::File::create(&out_path) {
                use std::io::Write;
                for r in &results {
                    let _ = writeln!(f, "{}", serde_json::to_string(r).unwrap_or_default());
                }
            }

            let mean = total / captures.len() as f32;
            eprintln!("\n=== Memory Quality Eval ===");
            eprintln!("mean_score={mean:.2}  n={}", captures.len());
            for r in &results {
                if r.judge_score < 0.60 {
                    eprintln!("  LOW [{}] {:.2} :: {}", r.capture_id, r.judge_score, r.judge_reason);
                }
            }

            assert!(
                hard_failures.is_empty(),
                "Hard failures (filename/template text): {hard_failures:?}"
            );
            assert!(
                mean >= 0.55,
                "Mean quality score {mean:.2} below threshold 0.55. See target/memory_quality_evals.jsonl"
            );
        });
    }
}
