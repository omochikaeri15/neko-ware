mod bntx;
mod csb;
mod surface;

use std::fs;
use std::path::{Path, PathBuf};

use colored::Colorize;
use image::RgbaImage;
use rayon::prelude::*;
use tracing::{debug, error, info, trace, warn};

use crate::fix::unpremultiply_pixels;
use crate::io::has_extension;

const TAB_SUFFIX: &str = ".tsv";
pub(crate) const TAB_SEPARATOR: &str = "\t";
const COMMA_SEPARATOR: &str = ",";

pub fn execute(targets: &[PathBuf], output_dir: Option<&str>, show_ui: bool) {
    let sources: Vec<&PathBuf> = targets
        .iter()
        .filter(|path| has_extension(path, "bntx") || has_extension(path, "csb"))
        .collect();

    if sources.is_empty() {
        if show_ui {
            println!("\n  {} No .bntx or .csb files found to convert\n", "!".yellow());
        }
        warn!("No convertible files found");
        std::process::exit(1);
    }

    let output_base = output_dir.map(PathBuf::from);

    if let Some(base) = &output_base
        && let Err(err) = fs::create_dir_all(base)
    {
        if show_ui {
            println!("\n  {} Could not create {}: {}\n", "✗".red(), base.display(), err);
        }
        error!(dest = %base.display(), error = %err, "Could not create output directory");
        std::process::exit(1);
    }

    if show_ui {
        println!(
            "\n  {} Converting {} files",
            "!".yellow(),
            sources.len().to_string().cyan()
        );
    }
    debug!(
        files = sources.len(),
        replace = output_base.is_none(),
        "Starting conversion"
    );

    let results: Vec<(&Path, Result<usize, String>)> = sources
        .par_iter()
        .with_max_len(1)
        .map(|source| {
            let outcome = convert_file(source, output_base.as_deref());
            (source.as_path(), outcome)
        })
        .collect();

    let mut converted_count = 0;
    let mut failed_count = 0;

    for (source, outcome) in results {
        match outcome {
            Ok(written) => {
                converted_count += written;
                trace!(file = %source.display(), written, "Converted file");
            }
            Err(err) => {
                failed_count += 1;
                if show_ui {
                    let display_name = source.file_name().unwrap_or_default().to_string_lossy();
                    println!("  {} Failed to convert {}: {}", "✗".red(), display_name.cyan(), err);
                }
                error!(file = %source.display(), error = %err, "Failed to convert file");
            }
        }
    }

    if converted_count == 0 {
        if show_ui {
            println!("\nFAILURE: Converted no files!\n");
        }
        error!(failed = failed_count, "Converted no files");
        std::process::exit(1);
    }

    if show_ui {
        println!("\nSUCCESS: Converted {} files!\n", converted_count.to_string().cyan());
    }
    info!(
        converted = converted_count,
        failed = failed_count,
        "Conversion complete"
    );
}

fn convert_file(source: &Path, output_base: Option<&Path>) -> Result<usize, String> {
    let bytes = fs::read(source).map_err(|err| err.to_string())?;
    let destination = output_base
        .map(Path::to_path_buf)
        .or_else(|| source.parent().map(Path::to_path_buf))
        .ok_or("Could not resolve an output directory")?;

    let stem = strip_container_extension(source);

    let written = if has_extension(source, "bntx") {
        write_textures(&bytes, &stem, &destination, false)?
    } else {
        write_table(&bytes, &stem, &destination)?
    };

    if output_base.is_none() {
        fs::remove_file(source).map_err(|err| err.to_string())?;
    }

    Ok(written)
}

pub fn repair_textures(source: &Path) -> Result<usize, String> {
    let bytes = fs::read(source).map_err(|err| err.to_string())?;
    let destination = source.parent().ok_or("Could not resolve an output directory")?;
    let stem = strip_container_extension(source);

    let written = write_textures(&bytes, &stem, destination, true)?;

    fs::remove_file(source).map_err(|err| err.to_string())?;

    Ok(written)
}

pub(crate) fn write_textures(bytes: &[u8], stem: &str, destination: &Path, unpremultiply: bool) -> Result<usize, String> {
    let textures = bntx::parse(bytes)?;

    if textures.is_empty() {
        return Err(String::from("Container holds no textures"));
    }

    let base = strip_suffix_ignore_case(stem, ".png").unwrap_or(stem);
    let mut written = 0;

    for (index, texture) in textures.iter().enumerate() {
        let mut pixels = surface::decode(texture)?;

        if unpremultiply {
            let touched = unpremultiply_pixels(&mut pixels);
            trace!(texture = %texture.name, pixels = touched, "Un-premultiplied decoded texture");
        }

        let width = u32::try_from(texture.width).map_err(|_| "Texture is impossibly wide")?;
        let height = u32::try_from(texture.height).map_err(|_| "Texture is impossibly tall")?;

        let image = RgbaImage::from_raw(width, height, pixels).ok_or("Decoded pixels do not fill the texture")?;

        let file_name = if index == 0 {
            format!("{base}.png")
        } else {
            format!("{base}_{}.png", sanitize(&texture.name))
        };

        image.save(destination.join(file_name)).map_err(|err| err.to_string())?;
        written += 1;
    }

    Ok(written)
}

pub(crate) fn write_table(bytes: &[u8], stem: &str, destination: &Path) -> Result<usize, String> {
    emit_table(bytes, stem, destination, false)?;

    Ok(1)
}

pub fn repair_table(source: &Path) -> Result<usize, String> {
    let bytes = fs::read(source).map_err(|err| err.to_string())?;
    let destination = source.parent().ok_or("Could not resolve an output directory")?;
    let stem = strip_container_extension(source);

    let flattened = emit_table(&bytes, &stem, destination, true)?;

    fs::remove_file(source).map_err(|err| err.to_string())?;

    Ok(flattened)
}

fn emit_table(bytes: &[u8], stem: &str, destination: &Path, normalize: bool) -> Result<usize, String> {
    let mut lines = csb::parse(bytes)?;

    let (file_name, separator) = if Path::new(stem).extension().is_some() {
        (stem.to_string(), name_separator(stem))
    } else {
        let (separator, extension) = infer_table_format(&lines);
        (format!("{stem}{extension}"), separator)
    };

    let mut flattened = 0;

    for value in lines.iter_mut().flatten() {
        if !value.contains(separator) && !value.contains(['\n', '\r']) {
            continue;
        }

        if !normalize {
            warn!(table = %stem, "Table holds a field containing its own separator or a line break");
            break;
        }

        *value = value
            .replace("\r\n", " ")
            .replace(['\n', '\r'], " ")
            .replace(separator, " ");
        flattened += 1;
    }

    if flattened > 0 {
        debug!(table = %stem, fields = flattened, "Flattened fields that would break naive parsing");
    }

    fs::write(destination.join(file_name), csb::render(&lines, separator)).map_err(|err| err.to_string())?;

    Ok(flattened)
}

pub(crate) fn name_separator(name: &str) -> &'static str {
    if strip_suffix_ignore_case(name, TAB_SUFFIX).is_some() {
        TAB_SEPARATOR
    } else {
        COMMA_SEPARATOR
    }
}

fn infer_table_format(lines: &[Vec<String>]) -> (&'static str, &'static str) {
    let mut holds_tab = false;
    let mut holds_comma = false;

    for value in lines.iter().flatten() {
        holds_tab |= value.contains('\t');
        holds_comma |= value.contains(',');
    }

    if holds_comma && !holds_tab {
        return (TAB_SEPARATOR, TAB_SUFFIX);
    }

    (COMMA_SEPARATOR, ".csv")
}

fn strip_container_extension(source: &Path) -> String {
    source
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned())
}

fn strip_suffix_ignore_case<'a>(value: &'a str, suffix: &str) -> Option<&'a str> {
    let split = value.len().checked_sub(suffix.len())?;
    let (head, tail) = value.split_at_checked(split)?;

    tail.eq_ignore_ascii_case(suffix).then_some(head)
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect()
}
