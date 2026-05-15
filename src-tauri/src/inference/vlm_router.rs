#[derive(Debug, Clone, PartialEq)]
pub enum VlmRouteDecision {
    SkipDuplicate,
    SkipGoodOcr,
    SkipLowValue,
    RunLightweightVlm,
    RunHeavyVlmExplicitOnly,
    FallbackOcrOnly { reason: String },
}

impl VlmRouteDecision {
    pub fn label(&self) -> &'static str {
        match self {
            Self::SkipDuplicate => "skip_duplicate",
            Self::SkipGoodOcr => "skip_good_ocr",
            Self::SkipLowValue => "skip_low_value",
            Self::RunLightweightVlm => "run_lightweight_vlm",
            Self::RunHeavyVlmExplicitOnly => "run_heavy_vlm_explicit",
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
        matches!(
            self,
            Self::RunLightweightVlm | Self::RunHeavyVlmExplicitOnly
        )
    }
}

pub struct VlmRouteInput<'a> {
    pub ocr_text_len: usize,
    pub ocr_confidence: f32,
    pub ocr_block_count: usize,
    pub visual_signal: bool,
    pub is_duplicate: bool,
    pub system_pressure_skip: bool,
    pub host_supports_vlm: bool,
    pub vlm_enabled: bool,
    pub vlm_model_id: Option<&'a str>,
    pub vlm_available: bool,
    pub vlm_calls_remaining: u32,
    pub vlm_timeout_secs: u64,
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

    if !input.host_supports_vlm {
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

    // Heavy VLM (Qwen3-VL 4B) only when explicitly requested — never default.
    if matches!(input.vlm_model_id, Some("qwen3-vl-4b")) {
        return VlmRouteDecision::RunHeavyVlmExplicitOnly;
    }

    VlmRouteDecision::RunLightweightVlm
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
            host_supports_vlm: true,
            vlm_enabled: true,
            vlm_model_id: Some("smolvlm-500m"),
            vlm_available: true,
            vlm_calls_remaining: 10,
            vlm_timeout_secs: 30,
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
    fn fallback_low_ram_before_model_load() {
        let mut inp = base_input();
        inp.host_supports_vlm = false;
        assert_eq!(
            should_run_vlm(&inp),
            VlmRouteDecision::FallbackOcrOnly {
                reason: "vlm_blocked_low_ram".to_string()
            }
        );
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
    fn run_lightweight_vlm_for_weak_ocr_frame() {
        let inp = base_input();
        assert_eq!(should_run_vlm(&inp), VlmRouteDecision::RunLightweightVlm);
    }

    #[test]
    fn heavy_vlm_only_when_explicitly_qwen() {
        let mut inp = base_input();
        inp.vlm_model_id = Some("qwen3-vl-4b");
        inp.ocr_text_len = 50;
        assert_eq!(
            should_run_vlm(&inp),
            VlmRouteDecision::RunHeavyVlmExplicitOnly
        );
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
    fn fallback_when_timeout_disabled() {
        let mut inp = base_input();
        inp.vlm_timeout_secs = 0;
        assert_eq!(
            should_run_vlm(&inp),
            VlmRouteDecision::FallbackOcrOnly {
                reason: "vlm_timeout_disabled".to_string()
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
}
