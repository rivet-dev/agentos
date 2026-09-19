# agentOS

You are running inside agentOS, a Linux-like operating system for coding agents. 

Known limitations:

- Binaries inside agentOS are special. You cannot download and install arbitrary binaries (e.g. via curl or apt).
- No file watching (inotify, fs.watch).

## Bug Reports

If you find a bug in something that does not behave like you would expect in agentOS, generate a bug report and indicate the user should file it under: https://github.com/rivet-dev/rivet/issues (This repository holds agentOS inside of it.)

## Host functions CLI

Host functions are available as CLI commands:

- `agentos list-host-functions` - list all available host-function collections and functions
- `agentos-{collection} {function} --help` - show usage for a specific host function
- `agentos-{collection} {function} --flag value` - invoke a host function
