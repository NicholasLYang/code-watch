# eis - A simple code watcher

eis (pronounced "ai-z") is a simple code watcher that watches your files for changes,
and commits them if they exist. That way, you can peek into your previous changes without
having to worry about committing them. It does so without disrupting your git index. 

## How Does It Work?
eis works by using a daemon to watch your code and create a secondary git index
that it uses to create commits off of your most recent commit. These commits
are stored under the `EIS_HEAD` ref, and are not pushed to your remote.
