//! PDF export helpers and commands.

use std::path::{Path, PathBuf};
use std::process::Command;

use genpdf::Element;

/// Path to the macOS Supplemental font directory where Arial ships.
const MACOS_SUPPLEMENTAL_FONT_DIR: &str = "/System/Library/Fonts/Supplemental";

/// PDF page margins applied uniformly to FNDR exports (millimetres).
const PDF_PAGE_MARGIN: u8 = 18;

/// Load the macOS Arial font family used by FNDR PDF exports.
fn load_pdf_font_family() -> Result<genpdf::fonts::FontFamily<genpdf::fonts::FontData>, String> {
    let font_dir = std::path::Path::new(MACOS_SUPPLEMENTAL_FONT_DIR);
    let load = |name: &str| {
        genpdf::fonts::FontData::load(font_dir.join(name), None)
            .map_err(|err| format!("Failed to load '{name}' from {font_dir:?}: {err}"))
    };
    Ok(genpdf::fonts::FontFamily {
        regular: load("Arial.ttf")?,
        bold: load("Arial Bold.ttf")?,
        italic: load("Arial Italic.ttf")?,
        bold_italic: load("Arial Bold Italic.ttf")?,
    })
}
/// Export a daily summary text to a PDF in the Downloads folder
#[tauri::command]
pub async fn export_daily_summary_pdf(
    _app: tauri::AppHandle,
    date_str: String,
    summary_text: String,
) -> Result<String, String> {
    // 1. Resolve Downloads folder
    let downloads_dir = dirs::download_dir()
        .ok_or_else(|| "Could not find Downloads folder on this system.".to_string())?;

    // 2. Prepare filename
    let safe_date = date_str.replace('/', "-").replace(' ', "_");
    let filename = format!("FNDR_Daily_Summary_{}.pdf", safe_date);
    let target_path = downloads_dir.join(filename);

    let mut doc = genpdf::Document::new(load_pdf_font_family()?);
    doc.set_title(format!("FNDR Daily Summary: {}", date_str));

    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(PDF_PAGE_MARGIN);
    doc.set_page_decorator(decorator);

    // Title & Header
    doc.push(
        genpdf::elements::Text::new("FNDR Daily Summary")
            .styled(genpdf::style::Style::new().bold().with_font_size(20)),
    );
    doc.push(
        genpdf::elements::Text::new(format!("Date: {}", date_str))
            .styled(genpdf::style::Style::new().with_font_size(10)),
    );
    doc.push(genpdf::elements::Break::new(1.5));

    if !summary_text.is_empty() {
        let mut list = genpdf::elements::UnorderedList::new();
        for line in summary_text.split('\n') {
            let trim = line.trim();
            if !trim.is_empty() {
                // Remove existing bullet points if present as genpdf adds its own
                let content = if trim.starts_with("- ")
                    || trim.starts_with("* ")
                    || trim.starts_with("• ")
                {
                    trim[2..].trim()
                } else if trim.starts_with("-") || trim.starts_with("*") || trim.starts_with("•")
                {
                    trim[1..].trim()
                } else {
                    trim
                };
                list.push(genpdf::elements::Paragraph::new(content.to_string()));
            }
        }
        doc.push(list);
    } else {
        doc.push(genpdf::elements::Paragraph::new(
            "No activity captured for this date.",
        ));
    }

    // Save
    doc.render_to_file(&target_path)
        .map_err(|e| format!("Failed to generate PDF file: {}", e))?;

    Ok(target_path.to_string_lossy().to_string())
}

/// Open a PDF exported by FNDR from the user's Downloads folder.
#[tauri::command]
pub async fn open_exported_pdf(path: String) -> Result<(), String> {
    let downloads_dir = dirs::download_dir()
        .ok_or_else(|| "Could not find Downloads folder on this system.".to_string())?;
    let downloads_dir = downloads_dir
        .canonicalize()
        .map_err(|e| format!("Could not resolve Downloads folder: {}", e))?;
    let target_path = PathBuf::from(path);
    let target_path = target_path
        .canonicalize()
        .map_err(|e| format!("Could not find exported PDF: {}", e))?;

    if !target_path.starts_with(&downloads_dir) {
        return Err("FNDR can only open exported PDFs from your Downloads folder.".to_string());
    }

    let is_pdf = target_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
    if !is_pdf {
        return Err("FNDR can only open exported PDF files.".to_string());
    }

    open_path_with_system(&target_path)
}

#[cfg(target_os = "macos")]
fn open_path_with_system(path: &Path) -> Result<(), String> {
    Command::new("open")
        .arg(path)
        .spawn()
        .map_err(|e| format!("Failed to open exported PDF: {}", e))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_path_with_system(path: &Path) -> Result<(), String> {
    Command::new("cmd")
        .arg("/C")
        .arg("start")
        .arg("")
        .arg(path)
        .spawn()
        .map_err(|e| format!("Failed to open exported PDF: {}", e))?;
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_path_with_system(path: &Path) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map_err(|e| format!("Failed to open exported PDF: {}", e))?;
    Ok(())
}
