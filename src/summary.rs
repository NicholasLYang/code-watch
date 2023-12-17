use crate::Watcher;

impl Watcher {
    pub fn summarize(&self) -> Result<(), anyhow::Error> {
        let Some(eis_head) = self.get_eis_head() else {
            println!("No history found, run `eis init`");
            return Ok(());
        };

        let mut eis_head_commit = self.repo.find_commit(eis_head)?;
        for _ in 0..10 {
            let parent = if eis_head_commit.parent_count() > 1 {
                eis_head_commit.parent(1)?
            } else {
                eis_head_commit.parent(0)?
            };
            println!("{}", eis_head_commit.id());
            let diff = self.repo.diff_tree_to_tree(
                Some(&parent.tree()?),
                Some(&eis_head_commit.tree()?),
                None,
            )?;
            diff.print(git2::DiffFormat::Patch, |delta, _hunk, line| {
                let line = String::from_utf8_lossy(line.content());
                let status = match delta.status() {
                    git2::Delta::Added => "+",
                    git2::Delta::Deleted => "-",
                    git2::Delta::Modified => "M",
                    git2::Delta::Renamed => "R",
                    git2::Delta::Copied => "C",
                    git2::Delta::Ignored => "I",
                    git2::Delta::Untracked => "U",
                    git2::Delta::Typechange => "T",
                    git2::Delta::Unreadable => "X",
                    git2::Delta::Conflicted => "!",
                    git2::Delta::Unmodified => " ",
                };
                print!("{} {}", status, line);
                true
            })?;
            eis_head_commit = parent;
        }

        Ok(())
    }
}
