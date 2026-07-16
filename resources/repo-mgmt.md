# PC project-store repository management

PC keeps generated project memory out of the subject repository:

```text
~/.pc/config.json               application configuration
~/.pc/projects/<project-id>/    portable canonical Git repository
~/.pc/state/<project-uuid>/     machine-local durable operational state
```

The project ID is a readable repository basename. If that name is already owned
by another subject repository, PC allocates `-1`, `-2`, and so on. Identity is a
stable UUID bound through the subject repository's Git common directory, so
linked worktrees share one store. Unrelated clones are not merged merely because
their basename or origin URL matches.

Use `pc project status` to inspect the binding and `pc project path` to print the
portable checkout. `pc project doctor` validates Git state, schema, identity, and
immutable objects.

## Sharing through a remote

The project store is a normal non-bare Git repository. You can—and generally
should—send it to a private remote if project memory should follow the repository
across computers or collaborators:

```bash
cd "$(pc project path)"
git remote add origin git@github.com:YOUR-ORG/YOUR-PC-MEMORY.git
git push -u origin master
```

Captured context can contain private source paths, design decisions, excerpts
from agent conversations, and evidence receipts. Do not publish a PC store to a
public remote unless that disclosure is intentional. PC never creates a remote
for you.

On another computer, clone the store directly under its manifest ID and attach
the new subject clone explicitly:

```bash
mkdir -p ~/.pc/projects
git clone git@github.com:YOUR-ORG/YOUR-PC-MEMORY.git ~/.pc/projects/<project-id>
cd /path/to/subject-repository
pc project attach ~/.pc/projects/<project-id>
pc project sync
```

Attachment is explicit by design. A matching Git origin is not proof that two
checkouts should share memory.

## Synchronization states

Every successful capture is committed locally before network work starts. With
no remote, capture remains fully functional and synchronization is inactive. PC
publishes an empty remote or missing upstream, pushes local-ahead history,
fast-forwards remote-ahead history, and retries transport/authentication failures
from durable local state.

True divergence is delegated to the configured trusted reconciliation command.
That agent receives the complete project-store repository and Git history and
owns fetch/pull, rebase or other integration, semantic conflict resolution,
commit, and push. PC does not preselect claims or author revision bodies. It does
set `PC_DISABLE_HOOKS=1` for the agent and all descendants, bound execution time
and logs, and then mechanically verify identity, schema, cleanliness, upstream
agreement, and preservation of every pre-existing immutable object.

Example `~/.pc/config.json` fragment:

```json
{
  "store_sync_enabled": true,
  "store_sync_poll_secs": 60,
  "store_sync_jitter_secs": 5,
  "store_remote": "origin",
  "store_branch": "master",
  "reconciliation_command": ["codex", "exec", "-"],
  "reconciliation_prompt_transport": "stdin",
  "reconciliation_timeout_secs": 900
}
```

For a CLI that needs the prompt as an argument, use
`"reconciliation_prompt_transport": "placeholder"` and include exactly one
`{prompt}` placeholder in an argv item. A wrapper script that reads stdin is the
portable adapter for other command shapes.

PC creates a new store on the configured `store_branch` (default `master`). If
you change that setting for an existing store, switch or rename the checkout's
current branch explicitly first; synchronization reports the mismatch as pending
and never merges into a different checked-out branch implicitly.

Logs are under
`~/.pc/state/<project-uuid>/logs/reconciliation/<attempt-id>/`. By default PC
keeps five attempts and at most 8 MiB each of stdout and stderr, draining and
discarding bytes beyond that cap so a noisy child cannot deadlock.

`pc project status` reports the durable inbox size, the latest synchronization
state/error, and the latest reconciliation attempt/postcondition result. Use the
attempt directory above for its bounded stdout, stderr, and complete result record.

## Transcript and provenance policy

The normalized raw harness transcript snapshot is machine-local queue input. It
stays under `~/.pc/state/<project-uuid>/capture-inbox/` until the corresponding
canonical commit is recoverably complete, then PC removes it; the raw transcript
is not copied wholesale into the portable store. Canonical capture manifests keep
the harness, source session ID, and transcript content hash rather than an absolute
transcript path.

Generated memory can still contain verbatim material selected from a conversation:
citation receipts and episode transcript sidecars are captured artifacts and may
be portable. Review that content before publishing a store, and prefer a private
remote. Machine-specific absolute transcript paths are neither identity nor a
portable provenance locator.

The current `pc configure` screen does not yet expose this complete repository
management surface. Its full UX refactor remains follow-up work; until then, edit
these synchronization and reconciliation fields in `~/.pc/config.json` directly.

## Legacy cutover

There is no automatic migration from `~/.proactive-context` or a subject
repository's `docs/wiki` directory. Recreate configuration and project memory
manually. Existing `docs/wiki` content has no special PC meaning after cutover:
PC never mutates it or treats it as canonical memory. If it is committed Markdown,
it remains eligible for ordinary subject-document indexing like any other docs.

The exact canonical directory nesting may evolve. Two invariants do not:
immutable objects are create-only, and every captured revision identifies its
parent revision. Machine-local inboxes, locks, databases, PIDs, logs, and retry
state remain outside the portable Git checkout.
