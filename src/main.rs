use colored::Colorize;
use comfy_table::{Cell, Color, ContentArrangement, Table};
use indicatif::{ProgressBar, ProgressStyle};
use number_prefix::NumberPrefix;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Instant;
use structopt::StructOpt;
use tokei::{Config, Languages};

#[derive(Debug, StructOpt)]
#[structopt(name = "loc-analyzer", about = "Analyze lines of code in a directory")]
struct Cli {
    /// The path to analyze (defaults to current directory)
    #[structopt(parse(from_os_str))]
    path: Option<PathBuf>,

    /// Optional second path for comparison
    #[structopt(parse(from_os_str))]
    compare_path: Option<PathBuf>,

    /// Show detailed breakdown including blank lines and comments
    #[structopt(short, long)]
    detailed: bool,

    /// Sort results by (code, files, comments)
    #[structopt(short, long, default_value = "code")]
    sort_by: String,

    /// Minimum lines of code to include in results
    #[structopt(long)]
    min_lines: Option<usize>,

    /// File extensions to exclude
    #[structopt(short, long)]
    exclude: Vec<String>,

    /// Output format (table, json, csv, html)
    #[structopt(short, long, default_value = "table")]
    format: String,

    /// Output file path
    #[structopt(short, long)]
    output: Option<PathBuf>,
}

#[derive(Clone, Serialize)]
struct FileStats {
    name: String,
    code: usize,
    comments: usize,
    blanks: usize,
    total: usize,
}

#[derive(Clone, Serialize)]
struct LanguageStats {
    name: String,
    files: Vec<FileStats>,
    file_count: usize,
    code: usize,
    comments: usize,
    blanks: usize,
    total: usize,
    percentage: f64,
}

#[derive(Clone, Serialize)]
struct AnalysisResult {
    timestamp: String,
    path: String,
    duration: f64,
    formatted_duration: String,
    total_files: usize,
    total_code: usize,
    total_comments: usize,
    total_blanks: usize,
    languages: Vec<LanguageStats>,
}

#[derive(Serialize)]
struct CsvRecord {
    name: String,
    file_count: usize,
    code_lines: usize,
    comment_lines: usize,
    blank_lines: usize,
    total_lines: usize,
    percentage: f64,
}

fn format_duration(seconds: f64) -> String {
    if seconds >= 3600.0 {
        // Hours
        format!("{:.3} hours", seconds / 3600.0)
    } else if seconds >= 60.0 {
        // Minutes
        format!("{:.3} minutes", seconds / 60.0)
    } else if seconds >= 1.0 {
        // Seconds
        format!("{:.3} seconds", seconds)
    } else if seconds >= 0.001 {
        // Milliseconds
        format!("{:.3} ms", seconds * 1000.0)
    } else {
        // Microseconds
        format!("{:.3} µs", seconds * 1_000_000.0)
    }
}

fn format_number(num: usize) -> String {
    match NumberPrefix::decimal(num as f64) {
        NumberPrefix::Standalone(n) => format!("{:.0}", n),
        NumberPrefix::Prefixed(p, n) => format!("{:.1}{}", n, p),
    }
}

fn calculate_percentage(part: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        ((part as f64 / total as f64) * 10000.0).round() / 100.0 // Round to 2 decimal places
    }
}

fn print_comparison(result1: &AnalysisResult, result2: &AnalysisResult) {
    println!("\n{}", "Directory Comparison".bold());

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.load_preset(comfy_table::presets::UTF8_FULL);

    table.set_header(vec!["Metric", "Directory 1", "Directory 2", "Difference"]);

    let metrics = vec![
        ("Total Files", result1.total_files, result2.total_files),
        ("Lines of Code", result1.total_code, result2.total_code),
        ("Comments", result1.total_comments, result2.total_comments),
        ("Blank Lines", result1.total_blanks, result2.total_blanks),
    ];

    for (name, val1, val2) in metrics {
        let diff = val2 as i64 - val1 as i64;
        let diff_str = if diff > 0 {
            format!("+{}", diff).green()
        } else {
            format!("{}", diff).red()
        };

        table.add_row(vec![
            Cell::new(name),
            Cell::new(format_number(val1)),
            Cell::new(format_number(val2)),
            Cell::new(diff_str),
        ]);
    }

    println!("{}", table);
}

fn analyze_directory(
    path: &PathBuf,
    config: &Config,
    pb: &ProgressBar,
    min_lines: Option<usize>,
    sort_by: &str,
) -> AnalysisResult {
    let start_time = Instant::now();
    let mut languages = Languages::new();

    // Analyze the directory
    languages.get_statistics(&[path], &[], config);
    pb.finish_and_clear();

    // Calculate totals
    let total_code: usize = languages.iter().map(|(_, l)| l.code).sum();
    let total_comments: usize = languages.iter().map(|(_, l)| l.comments).sum();
    let total_blanks: usize = languages.iter().map(|(_, l)| l.blanks).sum();
    let total_files: usize = languages.iter().map(|(_, l)| l.reports.len()).sum();

    // Convert to our statistics format
    let mut lang_stats: Vec<LanguageStats> = languages
        .iter()
        .map(|(lang, stats)| {
            let files = stats
                .reports
                .iter()
                .map(|report| FileStats {
                    name: report.name.to_string_lossy().to_string(),
                    code: report.stats.code,
                    comments: report.stats.comments,
                    blanks: report.stats.blanks,
                    total: report.stats.code + report.stats.comments + report.stats.blanks,
                })
                .collect::<Vec<FileStats>>();

            LanguageStats {
                name: lang.name().to_string(),
                file_count: files.len(),
                files,
                code: stats.code,
                comments: stats.comments,
                blanks: stats.blanks,
                total: stats.code + stats.comments + stats.blanks,
                percentage: calculate_percentage(stats.code, total_code),
            }
        })
        .filter(|stat| {
            if let Some(min) = min_lines {
                stat.code >= min
            } else {
                true
            }
        })
        .collect();

    // Apply sorting based on criteria
    lang_stats.sort_by(|a, b| {
        match sort_by {
            "files" => b.files.len().cmp(&a.files.len()),
            "comments" => b.comments.cmp(&a.comments),
            "blanks" => b.blanks.cmp(&a.blanks),
            "total" => b.total.cmp(&a.total),
            _ => b.code.cmp(&a.code), // Default sort by code
        }
    });

    AnalysisResult {
        timestamp: chrono::Local::now().to_rfc3339(),
        path: path.canonicalize().unwrap().to_string_lossy().to_string(),
        duration: start_time.elapsed().as_secs_f64(),
        formatted_duration: format_duration(start_time.elapsed().as_secs_f64()),
        total_files,
        total_code,
        total_comments,
        total_blanks,
        languages: lang_stats,
    }
}

fn generate_html(result: &AnalysisResult) -> String {
    let template = include_str!("templates/report.html");
    let mut handlebars = handlebars::Handlebars::new();
    handlebars
        .register_template_string("report", template)
        .unwrap();
    handlebars.render("report", result).unwrap()
}

fn main() {
    let args = Cli::from_args();

    let path = args
        .path
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    // Initialize config with exclusions
    let mut config = Config::default();
    if !args.exclude.is_empty() {
        config.types = Some(
            args.exclude
                .iter()
                .filter_map(|ext| tokei::LanguageType::from_file_extension(ext))
                .collect(),
        );
    }

    // Setup progress bar
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈"),
    );
    pb.set_message("Analyzing directory...");

    // Analyze directory
    let result = analyze_directory(&path, &config, &pb, args.min_lines, &args.sort_by);

    // Handle comparison if second path is provided
    if let Some(compare_path) = args.compare_path {
        let result2 = analyze_directory(&compare_path, &config, &pb, args.min_lines, &args.sort_by);
        print_comparison(&result, &result2);
        return;
    }

    // Handle output based on format
    match args.format.as_str() {
        "json" => {
            let json = serde_json::to_string_pretty(&result).unwrap();
            if let Some(output_path) = args.output {
                std::fs::write(output_path, json).unwrap();
            } else {
                println!("{}", json);
            }
        }
        "csv" => {
            if let Some(output_path) = args.output {
                let mut wtr = csv::Writer::from_path(output_path).unwrap();
                for lang in &result.languages {
                    let record = CsvRecord {
                        name: lang.name.clone(),
                        file_count: lang.files.len(),
                        code_lines: lang.code,
                        comment_lines: lang.comments,
                        blank_lines: lang.blanks,
                        total_lines: lang.total,
                        percentage: lang.percentage,
                    };
                    wtr.serialize(record).unwrap();
                }
                wtr.flush().unwrap();
            } else {
                let mut wtr = csv::Writer::from_writer(std::io::stdout());
                for lang in &result.languages {
                    let record = CsvRecord {
                        name: lang.name.clone(),
                        file_count: lang.files.len(),
                        code_lines: lang.code,
                        comment_lines: lang.comments,
                        blank_lines: lang.blanks,
                        total_lines: lang.total,
                        percentage: lang.percentage,
                    };
                    wtr.serialize(record).unwrap();
                }
                wtr.flush().unwrap();
            }
        }
        "html" => {
            let html = generate_html(&result);
            if let Some(output_path) = args.output {
                std::fs::write(output_path, html).unwrap();
            } else {
                println!("{}", html);
            }
        }
        _ => {
            // Default table output
            let mut table = Table::new();
            table.set_content_arrangement(ContentArrangement::Dynamic);
            table.load_preset(comfy_table::presets::UTF8_FULL);

            // Add headers based on detail level
            if args.detailed {
                table.set_header(vec![
                    "Language",
                    "Files",
                    "Code",
                    "Comments",
                    "Blanks",
                    "Total",
                    "% of Codebase",
                ]);
            } else {
                table.set_header(vec!["Language", "Files", "Lines of Code", "% of Codebase"]);
            }

            // Add rows
            for lang in &result.languages {
                if args.detailed {
                    table.add_row(vec![
                        Cell::new(&lang.name).fg(Color::Green),
                        Cell::new(lang.files.len()),
                        Cell::new(format_number(lang.code)),
                        Cell::new(format_number(lang.comments)),
                        Cell::new(format_number(lang.blanks)),
                        Cell::new(format_number(lang.total)),
                        Cell::new(format!("{:.1}%", lang.percentage)),
                    ]);
                } else {
                    table.add_row(vec![
                        Cell::new(&lang.name).fg(Color::Green),
                        Cell::new(lang.files.len()),
                        Cell::new(format_number(lang.code)),
                        Cell::new(format!("{:.1}%", lang.percentage)),
                    ]);
                }
            }

            // Print summary and table
            println!("\n{}", "Directory Analysis Summary".bold());
            println!("{}", table);

            // Print performance info
            println!("\n{}", "Performance:".bold());
            println!("  Time taken: {}\n", format_duration(result.duration));
        }
    }
}
