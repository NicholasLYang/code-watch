use anyhow::anyhow;
use clap::{Parser, Subcommand};
use git2::{Index, Oid, Repository};
use std::env::current_exe;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, process};
use sysinfo::{Pid, System, SystemExt};
use tokio::time::interval;

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Arguments {
    #[command(subcommand)]
    command: Command,
    /// Set current working directory
    #[arg(long)]
    cwd: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start watching the current repository
    Watch,
    /// Manually run the daemon
    Daemon,
    /// Initialize eis in the current repository
    Init,
    /// Check whether the current directory is an eis repository
    /// and whether the daemon is currently running
    Status,
}

fn is_daemon_running(pid_path: &Path) -> Result<bool, anyhow::Error> {
    if let Ok(pid) = fs::read_to_string(&pid_path) {
        let pid = pid.trim().parse::<Pid>()?;
        let system = System::new_all();
        if system.process(pid).is_some() {
            return Ok(true);
        }
    }

    Ok(false)
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = Arguments::parse();
    let cwd = match args.cwd {
        Some(cwd) => cwd,
        None => std::env::current_dir()?,
    };

    match args.command {
        Command::Watch => {
            if !cwd.join(".eis").exists() {
                return Err(anyhow!("eis is not initialized"));
            }

            let pid_path = cwd.join(".eis").join("daemon.pid");

            if is_daemon_running(&pid_path)? {
                println!("eis daemon is already running");
                return Ok(());
            }

            let bin = current_exe()?;

            let child = std::process::Command::new(bin)
                .current_dir(&cwd)
                .arg("daemon")
                .spawn()?;

            fs::write(pid_path, child.id().to_string())?;

            println!("Successfully started daemon");
            Ok(())
        }
        Command::Init => {
            if !cwd.join(".git").exists() {
                return Err(anyhow!(
                    "eis should be instantiated at the root of your git repository"
                ));
            }

            fs::create_dir_all(cwd.join(".eis"))?;

            let gitignore_path = cwd.join(".gitignore");
            if let Some(gitignore) = fs::read_to_string(&gitignore_path).ok() {
                if !gitignore.contains(".eis") {
                    fs::write(gitignore_path, format!("{}\n.eis", gitignore))?;
                }
            } else {
                fs::write(gitignore_path, ".eis")?;
            }

            println!("Successfully initialized eis");
            Ok(())
        }
        Command::Daemon => daemon(cwd).await,
        Command::Status => {
            if cwd.join(".eis").exists() {
                println!("eis is initialized");
                if is_daemon_running(&cwd.join(".eis").join("daemon.pid"))? {
                    println!("eis daemon is running");
                } else {
                    println!("eis daemon is not running");
                }
            } else {
                println!("eis is not initialized");
            }

            Ok(())
        }
    }
}

async fn daemon(cwd: PathBuf) -> Result<(), anyhow::Error> {
    let watcher = Watcher::new(&cwd)?;
    let mut interval = interval(Duration::from_secs(5));

    // Sets up ctrl-c handler so we can add the last changes before exiting
    ctrlc::set_handler(move || {
        let watcher = Watcher::new(&cwd).unwrap();
        watcher.watch().unwrap();
        process::exit(0);
    })?;

    loop {
        interval.tick().await;
        watcher.watch()?;
    }
}

const EIS_HEAD: &str = "EIS_HEAD";
struct Watcher {
    repo: Repository,
}

impl Watcher {
    fn new(path: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let repo = Repository::open(path.as_ref())?;

        Ok(Self { repo })
    }

    fn watch(&self) -> Result<(), anyhow::Error> {
        // Check if up to date and if not, we create a new one
        let eis_head = match self.get_eis_head() {
            Some(eis_head) if self.check_if_eis_head_is_up_to_date(eis_head)? => eis_head,
            _ => self.create_eis_head()?,
        };

        if let Some(tree) = self.create_tree()? {
            let eis_head_commit = self.repo.find_commit(eis_head)?;

            if tree != eis_head_commit.tree_id() {
                self.commit_tree(tree, eis_head)?;
            }
        }

        Ok(())
    }

    // Commits tree and updates `EIS_HEAD`
    fn commit_tree(&self, tree: Oid, parent: Oid) -> Result<Oid, anyhow::Error> {
        let tree = self.repo.find_tree(tree)?;
        let parent = self.repo.find_commit(parent)?;
        let signature = self.repo.signature()?;
        let message = "eis commit";
        let commit = self.repo.commit(
            Some(EIS_HEAD),
            &signature,
            &signature,
            message,
            &tree,
            &[&parent],
        )?;

        Ok(commit)
    }

    // Creates tree from temporary index of current repo state
    fn create_tree(&self) -> Result<Option<Oid>, anyhow::Error> {
        let index_file = Path::new(".git/eis-index");
        let mut index = Index::open(index_file)?;
        self.repo.set_index(&mut index)?;
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;

        if index.is_empty() {
            return Ok(None);
        }

        let oid = index.write_tree()?;
        // Clear up the index for next time
        index.clear()?;
        index.write()?;

        Ok(Some(oid))
    }

    // Checks if EIS_HEAD and HEAD have HEAD as a merge base. If not, then we need
    // to update EIS_HEAD to be off of HEAD
    fn check_if_eis_head_is_up_to_date(&self, eis_head: Oid) -> Result<bool, anyhow::Error> {
        let head = self.repo.head()?.target().unwrap();

        let merge_base = self.repo.merge_base(eis_head, head)?;

        Ok(merge_base == head)
    }

    // Creates the `EIS_HEAD` ref off of HEAD
    fn create_eis_head(&self) -> Result<Oid, anyhow::Error> {
        let head_id = self.repo.head()?.target().unwrap();
        let eis_head = self.repo.reference(EIS_HEAD, head_id, true, "eis head")?;

        Ok(eis_head.target().unwrap())
    }

    fn get_eis_head(&self) -> Option<Oid> {
        let head = self.repo.find_reference(&EIS_HEAD).ok()?;

        head.target()
    }
}
