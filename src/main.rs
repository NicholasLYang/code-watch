use daemonize::Daemonize;
use git2::{Index, Oid, Repository};
use std::fs::File;
use std::path::Path;
use std::process;
use std::time::Duration;
use tokio::time::interval;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let stdout = File::create("/tmp/daemon.out").unwrap();
    let stderr = File::create("/tmp/daemon.err").unwrap();

    let daemonize = Daemonize::new()
        .pid_file("/tmp/test.pid") // Every method except `new` and `start`
        .chown_pid_file(true) // is optional, see `Daemonize` documentation
        .working_directory("/tmp") // for default behaviour.
        .user("nobody")
        .group("daemon") // Group name
        .group(2) // or group id.
        .umask(0o777) // Set umask, `0o027` by default.
        .stdout(stdout) // Redirect stdout to `/tmp/daemon.out`.
        .stderr(stderr); // Redirect stderr to `/tmp/daemon.err`.

    daemonize.start()?;

    let watcher = Watcher::new(".")?;
    let mut interval = interval(Duration::from_secs(5));

    // Sets up ctrl-c handler so we can add the last changes before exiting
    /*ctrlc::set_handler(move || {
        let watcher = Watcher::new(".").unwrap();
        watcher.watch().unwrap();
        process::exit(0);
    })?;*/

    loop {
        interval.tick().await;
        watcher.watch()?;
    }
}

const CODE_WATCH_HEAD: &str = "CODE_WATCH_HEAD";
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
        let code_watch_head = match self.get_code_watch_head() {
            Some(code_watch_head)
                if self.check_if_code_watch_head_is_up_to_date(code_watch_head)? =>
            {
                code_watch_head
            }
            _ => self.create_code_watch_head()?,
        };

        if let Some(tree) = self.create_tree()? {
            let code_watch_head_commit = self.repo.find_commit(code_watch_head)?;

            if tree != code_watch_head_commit.tree_id() {
                self.commit_tree(tree, code_watch_head)?;
            }
        }

        Ok(())
    }

    // Commits tree and updates `CODE_WATCH_HEAD`
    fn commit_tree(&self, tree: Oid, parent: Oid) -> Result<Oid, anyhow::Error> {
        let tree = self.repo.find_tree(tree)?;
        let parent = self.repo.find_commit(parent)?;
        let signature = self.repo.signature()?;
        let message = "Code Watch Commit";
        let commit = self.repo.commit(
            Some(CODE_WATCH_HEAD),
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
        let index_file = Path::new(".git/code-watch-index");
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

    // Checks if CODE_WATCH_HEAD and HEAD have HEAD as a merge base. If not, then we need
    // to update CODE_WATCH_HEAD to be off of HEAD
    fn check_if_code_watch_head_is_up_to_date(
        &self,
        code_watch_head: Oid,
    ) -> Result<bool, anyhow::Error> {
        let head = self.repo.head()?.target().unwrap();

        let merge_base = self.repo.merge_base(code_watch_head, head)?;

        Ok(merge_base == head)
    }

    // Creates the `CODE_WATCH_HEAD` ref off of HEAD
    fn create_code_watch_head(&self) -> Result<Oid, anyhow::Error> {
        let head_id = self.repo.head()?.target().unwrap();
        let code_watch_head =
            self.repo
                .reference(CODE_WATCH_HEAD, head_id, true, "Code watch head")?;

        Ok(code_watch_head.target().unwrap())
    }

    fn get_code_watch_head(&self) -> Option<Oid> {
        let head = self.repo.find_reference(&CODE_WATCH_HEAD).ok()?;

        head.target()
    }
}
