use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use image::{RgbImage, imageops::FilterType};
use ultrahdr_core::{PixelFormat, RawImage, Unstoppable, gainmap::HdrOutputFormat};
use ultrajpeg::{
    CompressionEffort, DecodeOptions, DecodedImage, EncodeOptions, GainMapBundle, decode,
    decode_with_options, encode,
};

const REPORT_JPEG_QUALITY: u8 = 95;

fn main() {
    if let Err(error) = real_main() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<(), Box<dyn Error>> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask should live under the workspace root")
        .to_path_buf();

    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("release") => {
            if let Some(unexpected) = args.next() {
                return Err(format!("unexpected argument: {unexpected}").into());
            }
            release(&repo_root)
        }
        Some("report-fixtures") => {
            if let Some(unexpected) = args.next() {
                return Err(format!("unexpected argument: {unexpected}").into());
            }
            report_fixtures(&repo_root)
        }
        _ => Err(
            "usage:\n  cargo run -p xtask -- release\n  cargo run -p xtask -- report-fixtures"
                .into(),
        ),
    }
}

fn release(repo_root: &Path) -> Result<(), Box<dyn Error>> {
    ensure_clean_workdir(repo_root)?;

    let version = read_package_version(&repo_root.join("Cargo.toml"))?;
    let tag = format!("v{version}");

    run(Command::new("git")
        .current_dir(repo_root)
        .arg("tag")
        .arg(&tag))?;
    let push_status = Command::new("git")
        .current_dir(repo_root)
        .arg("push")
        .arg("origin")
        .arg(&tag)
        .status()?;
    if !push_status.success() {
        return Err(format!(
            "failed to push {tag} to origin; the local tag was created successfully. Push it manually with: git push origin {tag}"
        )
        .into());
    }

    Ok(())
}

fn read_package_version(path: &Path) -> Result<String, Box<dyn Error>> {
    let mut in_package = false;

    for line in fs::read_to_string(path)?.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }

        if !in_package {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "version" {
            continue;
        }

        let version = value.trim().trim_matches('"');
        if version.is_empty() {
            return Err(format!("package version is empty in {}", path.display()).into());
        }
        return Ok(version.to_owned());
    }

    Err(format!("could not find package.version in {}", path.display()).into())
}

fn ensure_clean_workdir(repo_root: &Path) -> Result<(), Box<dyn Error>> {
    let status = command_output(
        Command::new("git")
            .current_dir(repo_root)
            .arg("status")
            .arg("--short")
            .arg("--untracked-files=normal"),
    )?;

    if !status.trim().is_empty() {
        return Err("refusing to release from a dirty working tree".into());
    }

    Ok(())
}

fn report_fixtures(repo_root: &Path) -> Result<(), Box<dyn Error>> {
    let report_dir = repo_root.join("target/fixture-report");
    let assets_dir = report_dir.join("assets");
    if report_dir.exists() {
        fs::remove_dir_all(&report_dir)?;
    }
    fs::create_dir_all(&assets_dir)?;

    let fixtures = collect_fixture_paths(&repo_root.join("tests/fixtures/upstream"))?;
    if fixtures.is_empty() {
        return Err("no JPEG fixtures found under tests/fixtures".into());
    }

    let mut report = String::new();
    report.push_str(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>ultrajpeg fixture report</title>\
         <style>\
         :root{color-scheme:light;font-family:ui-sans-serif,system-ui,sans-serif;}\
         body{margin:2rem;background:#f5f5f2;color:#1c1c1c;}\
         h1,h2,h3{line-height:1.2;}\
         .summary{background:#fff;padding:1rem 1.25rem;border:1px solid #ddd;border-radius:12px;margin-bottom:1.5rem;}\
         .fixture{background:#fff;padding:1rem 1.25rem;border:1px solid #ddd;border-radius:12px;margin-bottom:1.5rem;}\
         table{border-collapse:collapse;width:100%;margin:0.75rem 0 1rem;}\
         th,td{border:1px solid #ddd;padding:0.5rem;text-align:left;vertical-align:top;}\
         th{background:#f0f0ec;}\
         .grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(280px,1fr));gap:1rem;margin-top:1rem;}\
         figure{margin:0;background:#fafaf7;border:1px solid #e0e0da;border-radius:10px;padding:0.75rem;}\
         img{display:block;max-width:100%;height:auto;background:#000;}\
         figcaption{margin-top:0.5rem;font-size:0.9rem;}\
         code{font-family:ui-monospace,SFMono-Regular,monospace;}\
         .ok{color:#0b6e4f;font-weight:600;}\
         .warn{color:#8a5b00;font-weight:600;}\
         </style></head><body>",
    );

    report.push_str("<h1>ultrajpeg Fixture Roundtrip Report</h1>");
    report.push_str(
        "<div class=\"summary\"><p>This report decodes every committed JPEG fixture, \
         re-encodes it through the public <code>ultrajpeg</code> API, decodes the result again, \
         and compares container sizes plus decoded image differences.</p>\
         <p>Encode policy used for this report: all report-generated JPEGs use quality 95. \
         Plain JPEG fixtures otherwise use the crate's default scan and compression settings; \
         Ultra HDR fixtures reuse decoded metadata and gain-map content, with primary options starting from \
         <code>EncodeOptions::ultra_hdr_defaults()</code> and gain-map JPEG defaults of sequential, balanced.</p>\
         <p>The original and re-encoded visuals are the actual JPEG assets. The only PNG files are normalized diff maps.</p>\
         <p>Diff images are normalized to the maximum absolute channel error for that comparison, so any non-zero difference remains visible.</p>\
         <p>Metrics are computed at full resolution; diff-map PNGs are downscaled when needed so the report stays usable.</p></div>",
    );

    let mut summary_rows = String::new();
    writeln!(
        summary_rows,
        "<table><thead><tr><th>Fixture</th><th>Kind</th><th>Assembled delta</th><th>Primary max abs</th><th>Gain-map max abs</th><th>Assembled max abs</th><th>Fast path</th></tr></thead><tbody>"
    )?;

    for (index, fixture_path) in fixtures.iter().enumerate() {
        let relative_path = fixture_path
            .strip_prefix(repo_root)
            .unwrap_or(fixture_path)
            .to_string_lossy()
            .replace('\\', "/");
        let slug = slugify(&relative_path, index);
        println!("analyzing {relative_path}");
        match analyze_fixture(repo_root, fixture_path, &report_dir, &assets_dir, &slug) {
            Ok(fixture) => {
                println!("finished {relative_path}");
                writeln!(
                    summary_rows,
                    "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    escape_html(&relative_path),
                    fixture.kind_label,
                    format_size_delta(fixture.reencoded_size as i64 - fixture.original_size as i64),
                    format_metric_value(fixture.primary_metrics.max_abs),
                    fixture.gain_map_metrics.as_ref().map_or_else(
                        || "n/a".to_string(),
                        |metrics| format_metric_value(metrics.max_abs)
                    ),
                    format_metric_value(fixture.assembled_metrics.max_abs),
                    fixture.fast_path_summary_html,
                )?;

                report.push_str(&fixture.html);
            }
            Err(error) => {
                println!("failed {relative_path}: {error}");
                writeln!(
                    summary_rows,
                    "<tr><td><code>{}</code></td><td colspan=\"6\"><span class=\"warn\">{}</span></td></tr>",
                    escape_html(&relative_path),
                    escape_html(&error.to_string()),
                )?;
                let partial = partial_fixture_html(
                    repo_root,
                    fixture_path,
                    &report_dir,
                    &assets_dir,
                    &slug,
                    &error.to_string(),
                )
                .unwrap_or_else(|_| fixture_error_html(&relative_path, &error.to_string()));
                report.push_str(&partial);
            }
        }
    }

    summary_rows.push_str("</tbody></table>");
    report.push_str("<div class=\"summary\"><h2>Summary</h2>");
    report.push_str(&summary_rows);
    report.push_str("</div>");
    report.push_str("</body></html>");

    fs::write(report_dir.join("index.html"), report)?;
    println!("wrote {}", report_dir.join("index.html").display());

    Ok(())
}

fn collect_fixture_paths(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut stack = vec![root.to_path_buf()];
    let mut paths = Vec::new();

    while let Some(dir) = stack.pop() {
        let original = dir.join("original.jpg");
        if original.is_file() {
            paths.push(original);
            continue;
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            let is_jpeg = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| matches!(ext, "jpg" | "jpeg" | "JPG" | "JPEG"));
            let is_downsampled = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("downsampled.jpg"));
            if is_jpeg && !is_downsampled {
                paths.push(path);
            }
        }
    }

    paths.sort();
    Ok(paths)
}

struct FixtureAnalysis {
    kind_label: String,
    original_size: usize,
    reencoded_size: usize,
    assembled_metrics: ImageMetrics,
    primary_metrics: ImageMetrics,
    gain_map_metrics: Option<ImageMetrics>,
    fast_path_summary_html: String,
    html: String,
}

fn fixture_error_html(path: &str, error: &str) -> String {
    format!(
        "<section class=\"fixture\">\
         <h2><code>{}</code></h2>\
         <p><span class=\"warn\">Analysis failed:</span> {}</p>\
         </section>",
        escape_html(path),
        escape_html(error),
    )
}

fn partial_fixture_html(
    repo_root: &Path,
    fixture_path: &Path,
    report_dir: &Path,
    assets_dir: &Path,
    slug: &str,
    error: &str,
) -> Result<String, Box<dyn Error>> {
    let bytes = fs::read(fixture_path)?;
    let relative_path = fixture_path
        .strip_prefix(repo_root)
        .unwrap_or(fixture_path)
        .to_string_lossy()
        .replace('\\', "/");
    let decoded = decode_for_report(&bytes)?;

    let assembled_original_path =
        write_binary_asset(assets_dir, slug, "assembled-original", "jpg", &bytes)?;
    let original_primary_bytes = decoded
        .primary_jpeg
        .as_ref()
        .ok_or("missing retained primary JPEG")?;
    let primary_original_path = write_binary_asset(
        assets_dir,
        slug,
        "primary-original",
        "jpg",
        original_primary_bytes,
    )?;
    let primary_reencoded = encode(
        &decoded.image,
        &EncodeOptions {
            quality: REPORT_JPEG_QUALITY,
            primary_metadata: decoded.primary_metadata.clone(),
            ..EncodeOptions::default()
        },
    )?;
    let primary_reencoded_path = write_binary_asset(
        assets_dir,
        slug,
        "primary-reencoded",
        "jpg",
        &primary_reencoded,
    )?;
    let primary_metrics = compare_rgb_images(
        &rgb_image_from_raw_image(&decode(original_primary_bytes)?.image)?,
        &rgb_image_from_raw_image(&decode(&primary_reencoded)?.image)?,
    )?;
    let primary_diff_path = write_rgb_png(
        assets_dir,
        slug,
        "primary-diff",
        &primary_metrics.diff_preview,
    )?;

    let gain_map_html = if let Some(gain_map) = decoded.gain_map.as_ref() {
        if let Some(original_gain_bytes) = gain_map.jpeg_bytes.as_ref() {
            let gain_original_path = write_binary_asset(
                assets_dir,
                slug,
                "gain-map-original",
                "jpg",
                original_gain_bytes,
            )?;
            let gain_reencoded = encode(
                &gain_map.image,
                &EncodeOptions {
                    quality: REPORT_JPEG_QUALITY,
                    ..EncodeOptions::default()
                },
            )?;
            let gain_reencoded_path = write_binary_asset(
                assets_dir,
                slug,
                "gain-map-reencoded",
                "jpg",
                &gain_reencoded,
            )?;
            let gain_metrics = compare_rgb_images(
                &rgb_image_from_raw_image(&decode(original_gain_bytes)?.image)?,
                &rgb_image_from_raw_image(&decode(&gain_reencoded)?.image)?,
            )?;
            let gain_diff_path = write_rgb_png(
                assets_dir,
                slug,
                "gain-map-diff",
                &gain_metrics.diff_preview,
            )?;
            comparison_section_html(
                report_dir,
                "Gain Map JPEG",
                "Original extracted gain-map codestream and plain-JPEG re-encode of the decoded gain-map image. Full HDR re-assembly remains unavailable for this fixture.",
                &gain_original_path,
                &gain_reencoded_path,
                &gain_diff_path,
                original_gain_bytes.len(),
                gain_reencoded.len(),
                &gain_metrics,
            )
        } else {
            "<h3>Gain Map JPEG</h3><p>Original gain-map codestream was not retained.</p>"
                .to_string()
        }
    } else {
        "<h3>Gain Map JPEG</h3><p>No gain-map codestream is available for this fixture.</p>"
            .to_string()
    };

    Ok(format!(
        "<section class=\"fixture\">\
         <h2><code>{}</code></h2>\
         <p><span class=\"warn\">Re-encoded side unavailable:</span> {}</p>\
         {}\
         {}\
         {}\
         </section>",
        escape_html(&relative_path),
        escape_html(error),
        original_only_section_html(
            report_dir,
            "Assembled JPEG",
            "Original assembled JPEG. The re-encoded side is unavailable for this fixture.",
            &assembled_original_path,
            bytes.len(),
        ),
        comparison_section_html(
            report_dir,
            "Primary JPEG",
            "Original extracted primary codestream and plain-JPEG re-encode of the decoded primary image.",
            &primary_original_path,
            &primary_reencoded_path,
            &primary_diff_path,
            original_primary_bytes.len(),
            primary_reencoded.len(),
            &primary_metrics,
        ),
        gain_map_html,
    ))
}

fn analyze_fixture(
    repo_root: &Path,
    fixture_path: &Path,
    report_dir: &Path,
    assets_dir: &Path,
    slug: &str,
) -> Result<FixtureAnalysis, Box<dyn Error>> {
    let bytes = fs::read(fixture_path)?;
    let relative_path = fixture_path
        .strip_prefix(repo_root)
        .unwrap_or(fixture_path)
        .to_string_lossy()
        .replace('\\', "/");
    let decoded = decode_for_report(&bytes)?;
    let reencoded = reencode_decoded(&decoded)?;
    let redecoded = decode_for_report(&reencoded)?;

    let assembled_original_path =
        write_binary_asset(assets_dir, slug, "assembled-original", "jpg", &bytes)?;
    let assembled_reencoded_path =
        write_binary_asset(assets_dir, slug, "assembled-reencoded", "jpg", &reencoded)?;
    let assembled_metrics = if decoded.gain_map.is_some() {
        let original_hdr = decoded.reconstruct_hdr(4.0, HdrOutputFormat::LinearFloat)?;
        let reencoded_hdr = redecoded.reconstruct_hdr(4.0, HdrOutputFormat::LinearFloat)?;
        compare_rgb_images(
            &rgb_image_from_raw_image(&original_hdr)?,
            &rgb_image_from_raw_image(&reencoded_hdr)?,
        )?
    } else {
        compare_rgb_images(
            &rgb_image_from_raw_image(&decoded.image)?,
            &rgb_image_from_raw_image(&redecoded.image)?,
        )?
    };
    let assembled_diff_path = write_rgb_png(
        assets_dir,
        slug,
        "assembled-diff",
        &assembled_metrics.diff_preview,
    )?;

    let original_primary_bytes = decoded
        .primary_jpeg
        .as_ref()
        .ok_or("missing retained primary JPEG")?;
    let reencoded_primary_bytes = redecoded
        .primary_jpeg
        .as_ref()
        .ok_or("missing retained primary JPEG")?;
    let primary_original_path = write_binary_asset(
        assets_dir,
        slug,
        "primary-original",
        "jpg",
        original_primary_bytes,
    )?;
    let primary_reencoded_path = write_binary_asset(
        assets_dir,
        slug,
        "primary-reencoded",
        "jpg",
        reencoded_primary_bytes,
    )?;
    let primary_metrics = compare_rgb_images(
        &rgb_image_from_raw_image(&decode(original_primary_bytes)?.image)?,
        &rgb_image_from_raw_image(&decode(reencoded_primary_bytes)?.image)?,
    )?;
    let primary_diff_path = write_rgb_png(
        assets_dir,
        slug,
        "primary-diff",
        &primary_metrics.diff_preview,
    )?;

    let (gain_map_metrics, gain_map_html) = if let (
        Some(original_gain_map),
        Some(reencoded_gain_map),
    ) =
        (decoded.gain_map.as_ref(), redecoded.gain_map.as_ref())
    {
        let original_gain_bytes = original_gain_map
            .jpeg_bytes
            .as_ref()
            .ok_or("missing retained gain-map JPEG")?;
        let reencoded_gain_bytes = reencoded_gain_map
            .jpeg_bytes
            .as_ref()
            .ok_or("missing retained gain-map JPEG")?;
        let gain_original_path = write_binary_asset(
            assets_dir,
            slug,
            "gain-map-original",
            "jpg",
            original_gain_bytes,
        )?;
        let gain_reencoded_path = write_binary_asset(
            assets_dir,
            slug,
            "gain-map-reencoded",
            "jpg",
            reencoded_gain_bytes,
        )?;
        let metrics = compare_rgb_images(
            &rgb_image_from_raw_image(&decode(original_gain_bytes)?.image)?,
            &rgb_image_from_raw_image(&decode(reencoded_gain_bytes)?.image)?,
        )?;
        let gain_diff_path =
            write_rgb_png(assets_dir, slug, "gain-map-diff", &metrics.diff_preview)?;
        let html = comparison_section_html(
            report_dir,
            "Gain Map JPEG",
            "Decoded gain-map JPEG pixels.",
            &gain_original_path,
            &gain_reencoded_path,
            &gain_diff_path,
            original_gain_bytes.len(),
            reencoded_gain_bytes.len(),
            &metrics,
        );
        (Some(metrics.without_diff_preview()), html)
    } else {
        (
            None,
            "<h3>Gain Map JPEG</h3><p>No gain-map codestream was available for this fixture comparison.</p>".to_string(),
        )
    };

    let fast_path_summary_html = if decoded.gain_map.is_some() {
        let fast_path = compare_fast_path_against_reference(&decoded)?;
        fast_path.summary_html()
    } else {
        "n/a".to_string()
    };
    let fast_path_detail_html = if decoded.gain_map.is_some() {
        let fast_path = compare_fast_path_against_reference(&decoded)?;
        fast_path.summary_detail_html()
    } else {
        String::new()
    };

    let assembled_note = if decoded.gain_map.is_some() {
        "The visuals below are the actual assembled JPEG files. The diff map compares reconstructed HDR output from the full container."
    } else {
        "The visuals below are the actual assembled JPEG files. The diff map compares decoded image pixels."
    };
    let html = format!(
        "<section class=\"fixture\">\
         <h2><code>{}</code></h2>\
         <table><tbody>\
         <tr><th>Container</th><td>{}</td></tr>\
         <tr><th>Primary format</th><td><code>{:?}</code> {}x{}</td></tr>\
         <tr><th>Fast path</th><td>{}</td></tr>\
         </tbody></table>\
         {}\
         {}\
         {}\
         <p>{}</p>\
         </section>",
        escape_html(&relative_path),
        if decoded.gain_map.is_some() {
            "Ultra HDR"
        } else {
            "JPEG"
        },
        decoded.image.format,
        decoded.image.width,
        decoded.image.height,
        fast_path_detail_html,
        comparison_section_html(
            report_dir,
            "Assembled JPEG",
            assembled_note,
            &assembled_original_path,
            &assembled_reencoded_path,
            &assembled_diff_path,
            bytes.len(),
            reencoded.len(),
            &assembled_metrics,
        ),
        comparison_section_html(
            report_dir,
            "Primary JPEG",
            "The raw primary codestream extracted from each assembled file.",
            &primary_original_path,
            &primary_reencoded_path,
            &primary_diff_path,
            original_primary_bytes.len(),
            reencoded_primary_bytes.len(),
            &primary_metrics,
        ),
        gain_map_html,
        if decoded.gain_map.is_some() {
            "Only the diff maps are PNGs. The original and re-encoded images above are the actual JPEG codestreams written to the report assets."
        } else {
            ""
        },
    );

    Ok(FixtureAnalysis {
        kind_label: if decoded.gain_map.is_some() {
            "Ultra HDR".to_string()
        } else {
            "JPEG".to_string()
        },
        original_size: bytes.len(),
        reencoded_size: reencoded.len(),
        assembled_metrics: assembled_metrics.without_diff_preview(),
        primary_metrics: primary_metrics.without_diff_preview(),
        gain_map_metrics,
        fast_path_summary_html,
        html,
    })
}

fn decode_for_report(bytes: &[u8]) -> Result<DecodedImage, Box<dyn Error>> {
    Ok(decode_with_options(
        bytes,
        DecodeOptions {
            decode_gain_map: true,
            retain_primary_jpeg: true,
            retain_gain_map_jpeg: true,
        },
    )?)
}

fn comparison_section_html(
    report_dir: &Path,
    title: &str,
    note: &str,
    original_path: &Path,
    reencoded_path: &Path,
    diff_path: &Path,
    original_size: usize,
    reencoded_size: usize,
    metrics: &ImageMetrics,
) -> String {
    format!(
        "<h3>{}</h3><p>{}</p>{}\
         <table><tbody>\
         <tr><th>Original size</th><td>{} bytes</td></tr>\
         <tr><th>Re-encoded size</th><td>{} bytes ({})</td></tr>\
         </tbody></table>\
         <div class=\"grid\">\
         <figure><img src=\"{}\" alt=\"original {}\"><figcaption>Original</figcaption></figure>\
         <figure><img src=\"{}\" alt=\"re-encoded {}\"><figcaption>Re-encoded</figcaption></figure>\
         <figure><img src=\"{}\" alt=\"{} diff map\"><figcaption>Normalized diff map</figcaption></figure>\
         </div>",
        escape_html(title),
        escape_html(note),
        metric_table_html(title, metrics),
        original_size,
        reencoded_size,
        format_size_delta(reencoded_size as i64 - original_size as i64),
        path_for_html(report_dir, original_path),
        escape_html(title),
        path_for_html(report_dir, reencoded_path),
        escape_html(title),
        path_for_html(report_dir, diff_path),
        escape_html(title),
    )
}

fn original_only_section_html(
    report_dir: &Path,
    title: &str,
    note: &str,
    original_path: &Path,
    original_size: usize,
) -> String {
    format!(
        "<h3>{}</h3><p>{}</p>\
         <table><tbody><tr><th>Original size</th><td>{} bytes</td></tr></tbody></table>\
         <div class=\"grid\">\
         <figure><img src=\"{}\" alt=\"original {}\"><figcaption>Original</figcaption></figure>\
         <figure><div class=\"warn\">Re-encoded side unavailable for this fixture.</div></figure>\
         </div>",
        escape_html(title),
        escape_html(note),
        original_size,
        path_for_html(report_dir, original_path),
        escape_html(title),
    )
}

fn write_binary_asset(
    assets_dir: &Path,
    slug: &str,
    stem: &str,
    extension: &str,
    bytes: &[u8],
) -> Result<PathBuf, Box<dyn Error>> {
    let path = assets_dir.join(format!("{slug}-{stem}.{extension}"));
    fs::write(&path, bytes)?;
    Ok(path)
}

fn reencode_decoded(decoded: &DecodedImage) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut options = if decoded.gain_map.is_some() {
        EncodeOptions::ultra_hdr_defaults()
    } else {
        EncodeOptions::default()
    };
    options.quality = REPORT_JPEG_QUALITY;
    options.primary_metadata = decoded.primary_metadata.clone();

    if let Some(gain_map) = decoded.gain_map.as_ref() {
        let metadata = gain_map
            .metadata
            .clone()
            .or_else(|| {
                decoded
                    .ultra_hdr
                    .as_ref()
                    .and_then(|metadata| metadata.gain_map_metadata.clone())
            })
            .ok_or("missing gain-map metadata for Ultra HDR fixture")?;
        options.gain_map = Some(GainMapBundle {
            image: gain_map.image.clone(),
            metadata,
            quality: REPORT_JPEG_QUALITY,
            progressive: false,
            compression: CompressionEffort::Balanced,
        });
    }

    Ok(encode(&decoded.image, &options)?)
}

#[derive(Clone)]
struct RgbFloatImage {
    width: u32,
    height: u32,
    pixels: Vec<[f32; 3]>,
}

#[derive(Clone)]
struct ImageMetrics {
    exact: bool,
    differing_pixels: usize,
    max_abs: f32,
    mean_abs: f32,
    rmse: f32,
    diff_preview: RgbFloatImage,
}

impl ImageMetrics {
    fn without_diff_preview(&self) -> Self {
        Self {
            exact: self.exact,
            differing_pixels: self.differing_pixels,
            max_abs: self.max_abs,
            mean_abs: self.mean_abs,
            rmse: self.rmse,
            diff_preview: RgbFloatImage {
                width: 0,
                height: 0,
                pixels: Vec::new(),
            },
        }
    }
}

struct FastPathComparison {
    linear: ImageMetrics,
    pq_exact: bool,
}

impl FastPathComparison {
    fn summary_html(&self) -> String {
        if self.linear.exact && self.pq_exact {
            "<span class=\"ok\">exact</span>".to_string()
        } else {
            format!(
                "<span class=\"warn\">linear max {}</span><br>pq exact: {}",
                format_metric_value(self.linear.max_abs),
                self.pq_exact
            )
        }
    }

    fn summary_detail_html(&self) -> String {
        format!(
            "Fast-path vs <code>ultrahdr-core</code> reference: linear exact = {}, pq exact = {}, linear max abs = {}, mean abs = {}",
            self.linear.exact,
            self.pq_exact,
            format_metric_value(self.linear.max_abs),
            format_metric_value(self.linear.mean_abs),
        )
    }
}

fn compare_fast_path_against_reference(
    decoded: &DecodedImage,
) -> Result<FastPathComparison, Box<dyn Error>> {
    let gain_map = decoded.gain_map.as_ref().ok_or("missing gain map")?;
    let metadata = gain_map
        .metadata
        .as_ref()
        .or_else(|| {
            decoded
                .ultra_hdr
                .as_ref()
                .and_then(|metadata| metadata.gain_map_metadata.as_ref())
        })
        .ok_or("missing gain-map metadata")?;

    let fast_linear = decoded.reconstruct_hdr(4.0, HdrOutputFormat::LinearFloat)?;
    let reference_linear = ultrahdr_core::gainmap::apply_gainmap(
        &decoded.image,
        &gain_map.gain_map,
        metadata,
        4.0,
        HdrOutputFormat::LinearFloat,
        Unstoppable,
    )?;
    let linear_metrics = compare_rgb_images(
        &rgb_image_from_raw_image(&fast_linear)?,
        &rgb_image_from_raw_image(&reference_linear)?,
    )?;

    let fast_pq = decoded.reconstruct_hdr(4.0, HdrOutputFormat::Pq1010102)?;
    let reference_pq = ultrahdr_core::gainmap::apply_gainmap(
        &decoded.image,
        &gain_map.gain_map,
        metadata,
        4.0,
        HdrOutputFormat::Pq1010102,
        Unstoppable,
    )?;

    Ok(FastPathComparison {
        linear: linear_metrics.without_diff_preview(),
        pq_exact: fast_pq.data == reference_pq.data,
    })
}

fn rgb_image_from_raw_image(image: &RawImage) -> Result<RgbFloatImage, Box<dyn Error>> {
    let width = image.width as usize;
    let height = image.height as usize;
    let stride = image.stride as usize;
    let mut pixels = Vec::with_capacity(width * height);

    match image.format {
        PixelFormat::Gray8 => {
            for y in 0..height {
                let row = &image.data[y * stride..y * stride + width];
                for &value in row {
                    let value = value as f32 / 255.0;
                    pixels.push([value, value, value]);
                }
            }
        }
        PixelFormat::Rgb8 => {
            for y in 0..height {
                let row = &image.data[y * stride..y * stride + width * 3];
                for pixel in row.chunks_exact(3) {
                    pixels.push([
                        pixel[0] as f32 / 255.0,
                        pixel[1] as f32 / 255.0,
                        pixel[2] as f32 / 255.0,
                    ]);
                }
            }
        }
        PixelFormat::Rgba8 => {
            for y in 0..height {
                let row = &image.data[y * stride..y * stride + width * 4];
                for pixel in row.chunks_exact(4) {
                    pixels.push([
                        pixel[0] as f32 / 255.0,
                        pixel[1] as f32 / 255.0,
                        pixel[2] as f32 / 255.0,
                    ]);
                }
            }
        }
        PixelFormat::Rgba32F => {
            for y in 0..height {
                let row = &image.data[y * stride..y * stride + width * 16];
                for pixel in row.chunks_exact(16) {
                    pixels.push([
                        f32::from_le_bytes(pixel[0..4].try_into()?),
                        f32::from_le_bytes(pixel[4..8].try_into()?),
                        f32::from_le_bytes(pixel[8..12].try_into()?),
                    ]);
                }
            }
        }
        PixelFormat::Rgba1010102Pq => {
            for y in 0..height {
                let row = &image.data[y * stride..y * stride + width * 4];
                for pixel in row.chunks_exact(4) {
                    let packed = u32::from_le_bytes(pixel.try_into()?);
                    let unpack = |shift| -> f32 { ((packed >> shift) & 0x3ff_u32) as f32 / 1023.0 };
                    pixels.push([unpack(0), unpack(10), unpack(20)]);
                }
            }
        }
        other => {
            return Err(format!("unsupported preview/diff format: {other:?}").into());
        }
    }

    Ok(RgbFloatImage {
        width: image.width,
        height: image.height,
        pixels,
    })
}

fn compare_rgb_images(
    left: &RgbFloatImage,
    right: &RgbFloatImage,
) -> Result<ImageMetrics, Box<dyn Error>> {
    if left.width != right.width || left.height != right.height {
        return Err("image dimensions differ".into());
    }
    if left.pixels.len() != right.pixels.len() {
        return Err("pixel counts differ".into());
    }

    let mut differing_pixels = 0usize;
    let mut max_abs = 0.0f32;
    let mut sum_abs = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut diff_pixels = Vec::with_capacity(left.pixels.len());

    for (a, b) in left.pixels.iter().zip(&right.pixels) {
        let diff = [
            (a[0] - b[0]).abs(),
            (a[1] - b[1]).abs(),
            (a[2] - b[2]).abs(),
        ];
        if diff[0] > 0.0 || diff[1] > 0.0 || diff[2] > 0.0 {
            differing_pixels += 1;
        }
        max_abs = max_abs.max(diff[0]).max(diff[1]).max(diff[2]);
        sum_abs += (diff[0] + diff[1] + diff[2]) as f64;
        sum_sq += (diff[0] * diff[0] + diff[1] * diff[1] + diff[2] * diff[2]) as f64;
        diff_pixels.push(diff);
    }

    let channel_count = (left.pixels.len() * 3) as f64;
    let scale = if max_abs > 0.0 { 1.0 / max_abs } else { 0.0 };
    let diff_preview = RgbFloatImage {
        width: left.width,
        height: left.height,
        pixels: diff_pixels
            .into_iter()
            .map(|diff| [diff[0] * scale, diff[1] * scale, diff[2] * scale])
            .collect(),
    };

    Ok(ImageMetrics {
        exact: differing_pixels == 0,
        differing_pixels,
        max_abs,
        mean_abs: (sum_abs / channel_count) as f32,
        rmse: (sum_sq / channel_count).sqrt() as f32,
        diff_preview,
    })
}

fn write_rgb_png(
    assets_dir: &Path,
    slug: &str,
    stem: &str,
    image: &RgbFloatImage,
) -> Result<PathBuf, Box<dyn Error>> {
    const REPORT_MAX_DIMENSION: u32 = 1600;

    let path = assets_dir.join(format!("{slug}-{stem}.png"));
    let mut bytes = Vec::with_capacity(image.pixels.len() * 3);
    for pixel in &image.pixels {
        bytes.push((pixel[0].clamp(0.0, 1.0) * 255.0).round() as u8);
        bytes.push((pixel[1].clamp(0.0, 1.0) * 255.0).round() as u8);
        bytes.push((pixel[2].clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    let png = RgbImage::from_raw(image.width, image.height, bytes)
        .ok_or("failed to build PNG image buffer")?;
    let (target_width, target_height) =
        report_dimensions(image.width, image.height, REPORT_MAX_DIMENSION);
    let png = if target_width != image.width || target_height != image.height {
        image::imageops::resize(&png, target_width, target_height, FilterType::CatmullRom)
    } else {
        png
    };
    png.save(&path)?;
    Ok(path)
}

fn report_dimensions(width: u32, height: u32, max_dimension: u32) -> (u32, u32) {
    if width <= max_dimension && height <= max_dimension {
        return (width, height);
    }

    let scale = (width.max(height) as f32) / max_dimension as f32;
    let new_width = ((width as f32) / scale).round().max(1.0) as u32;
    let new_height = ((height as f32) / scale).round().max(1.0) as u32;
    (new_width, new_height)
}

fn metric_table_html(label: &str, metrics: &ImageMetrics) -> String {
    format!(
        "<h3>{}</h3><table><tbody>\
         <tr><th>Exact match</th><td>{}</td></tr>\
         <tr><th>Differing pixels</th><td>{}</td></tr>\
         <tr><th>Max abs error</th><td>{}</td></tr>\
         <tr><th>Mean abs error</th><td>{}</td></tr>\
         <tr><th>RMSE</th><td>{}</td></tr>\
         </tbody></table>",
        escape_html(label),
        metrics.exact,
        metrics.differing_pixels,
        format_metric_value(metrics.max_abs),
        format_metric_value(metrics.mean_abs),
        format_metric_value(metrics.rmse),
    )
}

fn slugify(path: &str, index: usize) -> String {
    let mut slug = String::with_capacity(path.len() + 8);
    for ch in path.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else {
            slug.push('-');
        }
    }
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    format!("{index:02}-{slug}")
}

fn path_for_html(report_dir: &Path, path: &Path) -> String {
    path.strip_prefix(report_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn format_metric_value(value: f32) -> String {
    if value == 0.0 {
        "0".to_string()
    } else if value >= 1.0 {
        format!("{value:.6}")
    } else {
        format!("{value:.8}")
    }
}

fn format_size_delta(delta: i64) -> String {
    if delta == 0 {
        "0".to_string()
    } else {
        format!("{delta:+}")
    }
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn run(command: &mut Command) -> Result<(), Box<dyn Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("command {:?} failed with status {status}", command).into());
    }
    Ok(())
}

fn command_output(command: &mut Command) -> Result<String, Box<dyn Error>> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!("command {:?} failed with status {}", command, output.status).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}
