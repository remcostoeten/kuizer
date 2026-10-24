use super::AppError;
use crate::config::Config;
use crate::detect::Kind;

pub fn run(config: &Config, json: bool) -> Result<(), AppError> {
    let specs = config.rule_specs()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&specs)?);
        return Ok(());
    }
    println!(
        "{:<22}  {:<4}  {:<14}  {:<12}  {:<6}  {:<28}  CONTAINS",
        "NAME", "KIND", "PATTERN", "CATEGORY", "SAFETY", "MARKERS"
    );
    for spec in specs {
        let kind = match spec.kind {
            Kind::Dir => "dir",
            Kind::File => "file",
            Kind::External => "external",
        };
        println!(
            "{:<22}  {:<4}  {:<14}  {:<12}  {:<6}  {:<28}  {}",
            spec.name,
            kind,
            spec.pattern,
            spec.category.as_str(),
            spec.safety.as_str(),
            or_dash(&spec.markers),
            or_dash(&spec.contains),
        );
    }
    Ok(())
}

fn or_dash(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_owned()
    } else {
        values.join(", ")
    }
}
