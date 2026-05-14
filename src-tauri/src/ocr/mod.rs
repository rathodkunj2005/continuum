//! OCR module using Apple Vision framework

mod vision;

pub use vision::{text_volume_qualifies, OcrConfig, OcrEngine, RecognizedText};
