use std::path::{Path, PathBuf};

use colored::Colorize;
use rayon::prelude::*;
use tracing::{debug, error, info, trace, warn};

use crate::convert::{repair_table, repair_textures};
use crate::io::has_extension;

enum Repair {
    Texture,
    Image,
    Table,
}

#[derive(Default)]
struct Tally {
    repaired: usize,
    clean: usize,
    failed: usize,
    detail: usize,
}

pub struct FixKinds {
    pub png: bool,
    pub bntx: bool,
    pub csb: bool,
}

pub fn execute(targets: &[PathBuf], kinds: &FixKinds, show_ui: bool) {
    let sources: Vec<(&PathBuf, Repair)> = targets
        .iter()
        .filter_map(|path| {
            if kinds.bntx && has_extension(path, "bntx") {
                return Some((path, Repair::Texture));
            }
            if kinds.png && has_extension(path, "png") {
                return Some((path, Repair::Image));
            }
            if kinds.csb && has_extension(path, "csb") {
                return Some((path, Repair::Table));
            }
            None
        })
        .collect();

    if sources.is_empty() {
        if show_ui {
            println!("\n  {} No {} files found to fix\n", "!".yellow(), kinds.describe());
        }
        warn!("No repairable files found");
        std::process::exit(1);
    }

    if show_ui {
        println!("\n  {} Fixing {} files", "!".yellow(), sources.len().to_string().cyan());
    }
    debug!(files = sources.len(), "Starting repair pass");

    let results: Vec<(&Path, &Repair, Result<usize, String>)> = sources
        .par_iter()
        .with_max_len(1)
        .map(|(source, kind)| {
            let outcome = match kind {
                Repair::Texture => repair_textures(source),
                Repair::Image => unpremultiply_image(source),
                Repair::Table => repair_table(source),
            };
            (source.as_path(), kind, outcome)
        })
        .collect();

    let mut textures = Tally::default();
    let mut images = Tally::default();
    let mut tables = Tally::default();

    for (source, kind, outcome) in results {
        let tally = match kind {
            Repair::Texture => &mut textures,
            Repair::Image => &mut images,
            Repair::Table => &mut tables,
        };

        match outcome {
            Ok(0) => {
                tally.clean += 1;
                trace!(file = %source.display(), "Needed no correction");
            }
            Ok(detail) => {
                tally.repaired += 1;
                tally.detail += detail;
                trace!(file = %source.display(), detail, "Repaired file");
            }
            Err(err) => {
                tally.failed += 1;
                if show_ui {
                    let display_name = source.file_name().unwrap_or_default().to_string_lossy();
                    println!("  {} Failed to fix {}: {}", "✗".red(), display_name.cyan(), err);
                }
                error!(file = %source.display(), error = %err, "Failed to fix file");
            }
        }
    }

    if show_ui {
        if images.clean > 0 {
            println!(
                "  {} Skipped {} images with no semi-transparent pixels",
                "!".yellow(),
                images.clean.to_string().cyan()
            );
        }
        if tables.repaired > 0 {
            println!(
                "  {} Flattened {} fields across {} tables",
                "!".yellow(),
                tables.detail.to_string().cyan(),
                tables.repaired.to_string().cyan()
            );
        }
    }

    let settled = textures.settled() + images.settled() + tables.settled();

    if settled == 0 {
        if show_ui {
            println!("\nFAILURE: Fixed nothing!\n");
        }
        error!(
            failed = textures.failed + images.failed + tables.failed,
            "Fixed nothing"
        );
        std::process::exit(1);
    }

    if show_ui {
        let summary = [
            (textures.repaired, "textures"),
            (images.repaired, "images"),
            (tables.settled(), "tables"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, label)| format!("{} {label}", count.to_string().cyan()))
        .collect::<Vec<String>>();

        if summary.is_empty() {
            println!("\nSUCCESS: Nothing needed fixing!\n");
        } else {
            println!("\nSUCCESS: Fixed {}!\n", summary.join(", "));
        }
    }
    info!(
        textures = textures.repaired,
        images = images.repaired,
        images_clean = images.clean,
        tables = tables.settled(),
        fields = tables.detail,
        failed = textures.failed + images.failed + tables.failed,
        "Repair pass complete"
    );
}

pub fn unpremultiply_pixels(pixels: &mut [u8]) -> usize {
    let mut touched = 0;

    for texel in pixels.chunks_exact_mut(4) {
        let alpha = texel[3];

        if alpha == 0 || alpha == 255 {
            continue;
        }

        let scale = 255.0 / f64::from(alpha);
        let mut changed = false;

        for channel in &mut texel[..3] {
            let scaled = (f64::from(*channel) * scale).round_ties_even().min(255.0) as u8;
            changed |= scaled != *channel;
            *channel = scaled;
        }

        touched += usize::from(changed);
    }

    touched
}

fn unpremultiply_image(source: &Path) -> Result<usize, String> {
    let mut image = image::open(source).map_err(|err| err.to_string())?.to_rgba8();
    let touched = unpremultiply_pixels(&mut image);

    if touched == 0 {
        return Ok(0);
    }

    image.save(source).map_err(|err| err.to_string())?;

    Ok(touched)
}

impl Tally {
    fn settled(&self) -> usize {
        self.repaired + self.clean
    }
}

impl FixKinds {
    fn describe(&self) -> String {
        [(self.bntx, ".bntx"), (self.png, ".png"), (self.csb, ".csb")]
            .into_iter()
            .filter(|(wanted, _)| *wanted)
            .map(|(_, label)| label)
            .collect::<Vec<&str>>()
            .join(" or ")
    }
}
