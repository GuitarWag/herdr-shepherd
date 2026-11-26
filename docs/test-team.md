# Team test: calculator API

Use this test to check a large mixed team before a release.
One opencode boss and eight workers (four pi, four opencode) build a small Go REST API.

## Prepare

You need herdr, pi, opencode, Go and [adrlog](https://github.com/GuitarWag/adrlog) on PATH.
Use models that your account supports. The example uses the `yld` provider.

Create an empty repository:

```sh
mkdir -p ~/Downloads/personal/calc-api
git -C ~/Downloads/personal/calc-api init -b main
```

Add this template to `~/.config/herdr-shepherd/templates.yaml`:

```yaml
calc-api:
  repo: ~/Downloads/personal/calc-api
  boss: opencode
  workers:
    - pi:yld/glm-5.3-flash
    - pi:yld/inkling-2-mini
    - pi:yld/glm-5.3-flash
    - pi:yld/inkling-2-mini
    - opencode
    - opencode
    - opencode
    - opencode
```

## Run

Inside herdr, start the team and send the task to the boss:

```sh
cd ~/Downloads/personal/calc-api
herdr-shepherd --template calc-api
herdr-shepherd calc-api --send boss --message "$(cat <<'EOF'
Build a REST API for a calculator in Go, in this folder (an empty git repository).

Requirements:
- Four endpoints for the basic operations: add, subtract, multiply and divide.
- Each endpoint receives two numbers as parameters and returns the result as JSON.
- Return a clear error for invalid input and for division by zero.
- Use the Go standard library where it is enough. Include tests, and make go test ./... and go vet ./... pass.

Before any code, set up adrlog for this repository. adrlog is installed on PATH; run adrlog --help to see the commands. Record the first decisions with adrlog new, for example the HTTP routing, the parameter format, the response format and the error handling. Tell the workers to record their own decisions with adrlog new too.

Team rules:
- You have 8 workers, calc-api-w1 through calc-api-w8. Give editing workers separate files so they do not overwrite each other.
- Do not commit and do not push. When the work is done, run the tests yourself, review the code, and report the result in your pane.
EOF
)"
```

## Check

- Each worker pane shows several lines of output. With eight workers, the panes form two columns of four.
- Each worker waits idle until the boss sends a task. No worker runs a loop that polls the mailbox.
- Each worker reports with `herdr-shepherd calc-api --send boss`.
- `go test ./...` and `go vet ./...` pass.
- `adrlog list` shows records from the boss and from the workers.
- The boss stops any server that it starts for a smoke test.

## Clean up

Wait until every agent is idle, then close the `calc-api` tab.
opencode runs each session in its shared background service, so a closed tab does not stop a session that is still working.
If an agent still works, stop it first, or run `opencode service restart`.
Stop any server that the team left running, then remove the repository:

```sh
rm -rf ~/Downloads/personal/calc-api
```

## Results

2026-11-25, shepherd 0.5.0: the team built the API in about 13 minutes, and the tests and vet passed.
Seven workers delivered. One pi worker started its own mailbox polling loop and missed its task.
Five worker panes were two rows tall. The boss left its smoke test server running.
After the tab closed, an opencode session continued to work and rebuilt the deleted repository.

2026-11-26, shepherd 0.5.1: all eight workers delivered and reported through the mailbox in about 13 minutes.
Each worker pane had 10 or 11 rows. No worker polled the mailbox. go test and go vet passed, and adrlog had 12 records.
The boss stopped its own smoke test server.
