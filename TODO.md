# To Do

- prompts
    - next ticket
    - shortcut for /build -no-commit
    - commit
- get rid of sequenced id?
- see `tz-39` for other open questions
- list effort in `tz ready`
- tui:
    - color-coded efforts
    - no epics in orphan list, maybe a separate list for specs?
- tz stats doesn't show a triage count.



I would like to form and document a agent-based workflow using `tazk`. I will provide what i see as the key components of the workflow. First, please ask me questions to resolve ambiguities, fill in gaps and challenge where you see fit. Once we come to a common understanding, we will create a markdown document outlining the workflow. The document will serve as the source for understanding a certain way to use this tool, a guide for both humans and agents, and will inform the creation of agent-facing docs (skills, `AGENT.md` files). This should not feel like "the" way to use the tool, but just one way it can be used that fits a certain use case.

This workflow fits the use case of a single human directing AI agents to spec, build and triage development tasks. The primary goal is to be flexible. It should be able to handle small one-off tasks with no planning phase, as well as multi-epic dev cycles. It should not mandate a process, but respond to the user's needs. It should allow easy capture, and agent/human workflows. It should provide enough structure (roles, blocking, acceptance criteria, cli interface) that agents have the clarity to exercise the autonomy that makes them most efficient.

## Things to support

- `spec` type tickets capture the output of planning. rather than creating files in `docs/` for every planning cycle, `spec` tickets hold them in the issue tracker, which means they can be closed, linked to the work they spec'ed, but still persisted.
- the `role` property makes it clear whether the task is for a human or an agent. anything open and unblocked and with the `agent` role is fair game for an agent to pick up. it also allows agent tasks to block on asks from the human. and the human can easily see their own deliverables. the `role` property was intentionally made distinct from `assignee`, made a requirement and given only its two valid values to make this a first-class part of the process. `assignee` can be informational (name of human, model of agent), used for tracking and auditing, or for ensuring ownership. it's a field with flexibility, and that is good. but making `role` orthogonal to `assignee` allows it to be flexible while allowing a process that is concrete about whether a human or an agent owns the task. this enables strong workflow boundaries that actually simplify the workflow for everyone.
- the triage list intentionally does not allow parameters (assignment, priorities, dependencies). it's an inbox for findings that enforces that the finder process does not distract itself with planning. a reviewer, validator, or builder that encounters a bug by chance can dump a description in triage, without getting sidetracked with analyzing if/how/when it should
be addressed.
- a triage process that can be triggered (via a skill) at will by the user. the process evaluates triage items for conversion to tasks. since it is a dedicated process that will not be doing implementation itself, it can focus its context on evaluating the tickets, validating them, consolidating and deduping them, assigning role, effort, priority, and grouping them into epics where that makes sense.
- `tz ready -r agent` is the entry point for a builder agent. anything it returns is both a work ticket (excludes epics and specs), open, and unblocked.
- this workflow is intended to be non-parallel. `tz` does not have non-local locks.
- issues should be claimed before starting work and closed when done.
- the cli is the entry point, it validates the workflow.
- when an agent needs something from the human that isn't easily asked in chat flow, the mechanism to record this is by a task with `role` = `human` with `blocked_by` relationships set on any tasks the ask blocks. an agent doing planning or spec work can do this directly. an implementing agent can create a triage item, noting clearly that the deliverable is for a human, and what it blocks. this can be canonized into a task by the triage process.
- `effort` can be used to route agent/thinking choice. if we do so, it needs a flexible map to track model updates. not necessary for v1.

## Skills

### /build
- `tz ready -r agent`
- variant to commit or not?
- don't push
- claim ticket, assign model/effort as assignee (sandbox id/pi session id?)
- close the ticket
- effort mapping? model, remaining context, effort
- choose by priority otherwise

### /triage
- go through triage items and turn into tickets
    - assess validity
    - combine/dedupe
    - group into epics if needed
    - assign role/effort/priority

### /issue-hygeine
- close epics whose tasks are done
- propagate blocks to epic tasks

### /to-tickets
- turn a spec (document or conversation) into concrete `tz` issues
- breaks work into epics where needed
- create blocks between issues where needed
- include acceptance criteria for all issues
- assign role and effort
