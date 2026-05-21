/// Decision produced by [`should_run_vlm`].
#[derive(Debug, Clone, PartialEq)]
pub enum VlmRouteDecision {
    SkipDuplicate,
    SkipGoodOcr,
    SkipLowValue,
    /// Run Qwen3-VL-2B for this frame.
    RunQwenVlm,
    FallbackOcrOnly {
        reason: String,
    },
}

impl VlmRouteDecision {
    pub fn label(&self) -> &'static str {
        match self {
            Self::SkipDuplicate => "skip_duplicate",
            Self::SkipGoodOcr => "skip_good_ocr",
            Self::SkipLowValue => "skip_low_value",
            Self::RunQwenVlm => "run_qwen_vlm",
            Self::FallbackOcrOnly { .. } => "fallback_ocr_only",
        }
    }

    pub fn fallback_reason(&self) -> Option<&str> {
        match self {
            Self::FallbackOcrOnly { reason } => Some(reason.as_str()),
            _ => None,
        }
    }

    pub fn runs_pixel_vlm(&self) -> bool {
        matches!(self, Self::RunQwenVlm)
    }
}

pub struct VlmRouteInput<'a> {
    pub ocr_text_len: usize,
    pub ocr_confidence: f32,
    pub ocr_block_count: usize,
    pub visual_signal: bool,
    pub is_duplicate: bool,
    pub system_pressure_skip: bool,
    /// Host has ≥ 8 GB RAM — safe to run Qwen3-VL-2B (~3.5 GB usage).
    pub host_supports_qwen_vlm: bool,
    pub vlm_enabled: bool,
    pub vlm_available: bool,
    pub vlm_calls_remaining: u32,
    pub vlm_timeout_secs: u64,
    // Phantom lifetime to allow callers that pass &str fields without needing
    // to restructure call sites.
    pub _phantom: std::marker::PhantomData<&'a ()>,
}

pub fn should_run_vlm(input: &VlmRouteInput) -> VlmRouteDecision {
    if input.is_duplicate {
        return VlmRouteDecision::SkipDuplicate;
    }
    if !input.vlm_enabled {
        return VlmRouteDecision::FallbackOcrOnly {
            reason: "vlm_disabled".to_string(),
        };
    }
    if !input.host_supports_qwen_vlm {
        return VlmRouteDecision::FallbackOcrOnly {
            reason: "vlm_blocked_low_ram".to_string(),
        };
    }
    if input.system_pressure_skip {
        return VlmRouteDecision::FallbackOcrOnly {
            reason: "system_pressure".to_string(),
        };
    }
    // Good OCR: VLM adds diminishing returns when text is rich.
    if input.ocr_text_len >= 300 && input.ocr_block_count >= 10 && input.ocr_confidence >= 0.40 {
        return VlmRouteDecision::SkipGoodOcr;
    }
    // Low value: almost nothing to analyze visually.
    if !input.visual_signal && input.ocr_text_len < 60 && input.ocr_block_count < 3 {
        return VlmRouteDecision::SkipLowValue;
    }
    if input.vlm_timeout_secs == 0 {
        return VlmRouteDecision::FallbackOcrOnly {
            reason: "vlm_timeout_disabled".to_string(),
        };
    }
    if input.vlm_calls_remaining == 0 {
        return VlmRouteDecision::FallbackOcrOnly {
            reason: "vlm_rate_limited".to_string(),
        };
    }
    if !input.vlm_available {
        return VlmRouteDecision::FallbackOcrOnly {
            reason: "vlm_unavailable".to_string(),
        };
    }
    VlmRouteDecision::RunQwenVlm
}

pub fn vlm_capability_label(
    vlm_enabled: bool,
    host_supports_qwen_vlm: bool,
    vlm_available: bool,
) -> &'static str {
    if !vlm_enabled {
        "disabled"
    } else if !host_supports_qwen_vlm {
        "unsupported"
    } else if !vlm_available {
        "model_missing"
    } else {
        "available"
    }
}

pub fn vlm_runtime_status_label(
    decision: &VlmRouteDecision,
    pressure_reason: Option<&str>,
) -> &'static str {
    match decision {
        VlmRouteDecision::RunQwenVlm => "ready",
        VlmRouteDecision::FallbackOcrOnly { reason } if reason == "vlm_blocked_low_ram" => {
            "unsupported"
        }
        VlmRouteDecision::FallbackOcrOnly { reason } if reason == "system_pressure" => {
            match pressure_reason.unwrap_or_default() {
                "host_memory_high" | "host_memory_moderate" | "process_footprint_over_3gib" => {
                    "deferred_low_ram"
                }
                "process_cpu_saturated" => "deferred_pressure",
                _ => "deferred_pressure",
            }
        }
        VlmRouteDecision::FallbackOcrOnly { reason } if reason == "vlm_unavailable" => {
            "model_missing"
        }
        VlmRouteDecision::FallbackOcrOnly { reason } if reason == "vlm_disabled" => "disabled",
        VlmRouteDecision::FallbackOcrOnly { reason } if reason == "vlm_rate_limited" => {
            "deferred_rate_limited"
        }
        VlmRouteDecision::FallbackOcrOnly { .. } => "deferred",
        VlmRouteDecision::SkipDuplicate
        | VlmRouteDecision::SkipGoodOcr
        | VlmRouteDecision::SkipLowValue => "skipped",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_input() -> VlmRouteInput<'static> {
        VlmRouteInput {
            ocr_text_len: 100,
            ocr_confidence: 0.48,
            ocr_block_count: 8,
            visual_signal: true,
            is_duplicate: false,
            system_pressure_skip: false,
            host_supports_qwen_vlm: true,
            vlm_enabled: true,
            vlm_available: true,
            vlm_calls_remaining: 10,
            vlm_timeout_secs: 30,
            _phantom: std::marker::PhantomData,
        }
    }

    #[test]
    fn skip_duplicate() {
        let mut inp = base_input();
        inp.is_duplicate = true;
        assert_eq!(should_run_vlm(&inp), VlmRouteDecision::SkipDuplicate);
    }

    #[test]
    fn skip_good_ocr() {
        let mut inp = base_input();
        inp.ocr_text_len = 600;
        inp.ocr_confidence = 0.50;
        inp.ocr_block_count = 20;
        assert_eq!(should_run_vlm(&inp), VlmRouteDecision::SkipGoodOcr);
    }

    #[test]
    fn skip_low_value_tiny_frame() {
        let mut inp = base_input();
        inp.ocr_text_len = 30;
        inp.ocr_block_count = 1;
        inp.visual_signal = false;
        assert_eq!(should_run_vlm(&inp), VlmRouteDecision::SkipLowValue);
    }

    #[test]
    fn fallback_low_ram() {
        let mut inp = base_input();
        inp.host_supports_qwen_vlm = false;
        assert_eq!(
            should_run_vlm(&inp),
            VlmRouteDecision::FallbackOcrOnly {
                reason: "vlm_blocked_low_ram".to_string()
            }
        );
    }

    #[test]
    fn run_qwen_for_weak_ocr_frame() {
        assert_eq!(should_run_vlm(&base_input()), VlmRouteDecision::RunQwenVlm);
    }

    #[test]
    fn fallback_system_pressure() {
        let mut inp = base_input();
        inp.system_pressure_skip = true;
        assert!(matches!(
            should_run_vlm(&inp),
            VlmRouteDecision::FallbackOcrOnly { .. }
        ));
    }

    #[test]
    fn fallback_vlm_disabled() {
        let mut inp = base_input();
        inp.vlm_enabled = false;
        assert!(matches!(
            should_run_vlm(&inp),
            VlmRouteDecision::FallbackOcrOnly { .. }
        ));
    }

    #[test]
    fn fallback_when_budget_exhausted() {
        let mut inp = base_input();
        inp.vlm_calls_remaining = 0;
        assert_eq!(
            should_run_vlm(&inp),
            VlmRouteDecision::FallbackOcrOnly {
                reason: "vlm_rate_limited".to_string()
            }
        );
    }

    #[test]
    fn fallback_when_model_unavailable() {
        let mut inp = base_input();
        inp.vlm_available = false;
        assert_eq!(
            should_run_vlm(&inp),
            VlmRouteDecision::FallbackOcrOnly {
                reason: "vlm_unavailable".to_string()
            }
        );
    }

    #[test]
    fn capability_is_available_even_when_runtime_pressure_defers() {
        let mut inp = base_input();
        inp.system_pressure_skip = true;
        let decision = should_run_vlm(&inp);

        assert_eq!(
            vlm_capability_label(
                inp.vlm_enabled,
                inp.host_supports_qwen_vlm,
                inp.vlm_available
            ),
            "available"
        );
        assert_eq!(
            vlm_runtime_status_label(&decision, Some("host_memory_moderate")),
            "deferred_low_ram"
        );
    }

    #[test]
    fn capability_distinguishes_missing_disabled_and_unsupported() {
        assert_eq!(vlm_capability_label(false, true, true), "disabled");
        assert_eq!(vlm_capability_label(true, false, true), "unsupported");
        assert_eq!(vlm_capability_label(true, true, false), "model_missing");
        assert_eq!(vlm_capability_label(true, true, true), "available");
    }
}
