use super::AppError;
use super::removal::{self, Request};
use crate::cli::DockerArgs;
use crate::config::Config;
use crate::delete::Mode;
use crate::docker::{self, Object};
use crate::output;
use crate::scan::{Candidate, Issue, IssueKind};

pub fn run(config: &Config, args: DockerArgs, json: bool, quiet: bool) -> Result<(), AppError> {
    let names: Vec<&str> = Object::ALL.iter().map(|object| object.name()).collect();
    removal::check_selection(&args.removal.select, &names)?;
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
    let entries = docker::entries(listing.items);
    let cleaning = args.clean || args.removal.dry_run;
    if !cleaning {
        if json {
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
    let selected = removal::select(candidates, &args.removal.select);
    let request = Request {
        command: "docker",
        roots: &[],
        mode: if args.removal.dry_run {
            Mode::DryRun
        } else {
            Mode::Permanent
        },
        yes: args.removal.yes,
        json,
        remove: docker::remove,
    };
    removal::run(&request, selected, &issues)
}
