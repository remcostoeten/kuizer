use super::AppError;
use super::filter::Filters;
use super::removal::{self, Request};
use crate::cli::{DockerArgs, Format};
use crate::config::Config;
use crate::docker::{self, Object};
use crate::output;
use crate::scan::{Candidate, Issue, IssueKind};
use crate::source::Source;

pub fn run(config: &Config, args: DockerArgs, format: Format, quiet: bool) -> Result<(), AppError> {
    let json = format != Format::Text;
    let plan = args.plan();
    let names: Vec<&str> = Object::ALL.iter().map(|object| object.name()).collect();
    removal::check_selection(&plan.removal.select, &names)?;
    let filters = Filters::source(config, &args.filters, &names)?;
    if !quiet && !json {
        output::print_status("asking docker and podman");
    }
    let listing = docker::list(
        &config.docker.engines,
        config.docker.timeout(),
        config.docker.recent_hours,
    );
    if !quiet && !json {
        output::clear_progress();
    }
    let mut entries = docker::entries(listing.items);
    filters.keep(&mut entries);
    if !plan.cleaning {
        if format == Format::JsonLines {
            println!("{}", output::docker_lines(&entries, &listing.issues)?);
        } else if json {
            println!("{}", output::docker_report(&entries, &listing.issues)?);
        } else {
            output::print_docker(&entries, &listing.issues, quiet);
        }
        return Ok(());
    }
    let issues: Vec<Issue> = listing
        .issues
        .iter()
        .map(|message| Issue {
            path: "docker".into(),
            kind: IssueKind::Io,
            message: message.clone(),
        })
        .collect();
    if !json {
        for issue in &issues {
            eprintln!("kuizer: {}", issue.message);
        }
    }
    let candidates: Vec<Candidate> = entries
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect();
    let selected = removal::select(candidates, &plan.removal.select);
    let request = Request {
        command: "docker",
        roots: &[],
        mode: Source::Docker.mode(removal::mode(&plan.removal, config)),
        yes: plan.removal.yes,
        confirm: config.delete.confirm,
        json,
        source: Source::Docker,
        engines: &config.docker.engines,
        delete: &config.delete,
    };
    removal::run(&request, selected, &issues)
}
