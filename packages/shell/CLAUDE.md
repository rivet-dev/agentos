# agentos-shell

- Do not implement or route through a custom/synthetic shell, prompt, line editor, or command parser; interactive shell mode must launch native Bash through the terminal/PTY path so behavior matches `docker run -it bash`.
- Keep `agentos-shell` loading every command-providing package from secure-exec `software/`; when that registry changes, update the imports, package dependencies, and smoke coverage here.
