# Workflow: Solo-Directed Agents

One way to use `tz`. It suits a single human directing AI agents who write
specs, build, and triage development work. It is not the way to use `tz`. Other
setups (teams, parallel agents, no agents at all) would make different choices.

This document is for both the human and the agents. Skills and `AGENTS.md`
files for this workflow are derived from it. Where they disagree, this document
wins.

## Goals

- **Flexible.** The same tools handle a one-line fix with no planning and a
  multi-epic dev cycle. The workflow responds to what the human asks for. It
  doesn't impose a process.
- **Easy capture.** Recording a finding costs a single command and no planning.
- **Structure that frees agents to act.** Roles, blockers, acceptance criteria
  and the CLI give agents clear boundaries, so they can act without checking in
  at every step.

## Ground rules

These apply to every agent in this workflow, whatever skill it is running.

1. **Writes go through `tz`.** The CLI checks every change. Agents may *read*
   `.tazk/` files directly, which is handy for long specs. They never edit them
   by hand.
2. **One agent works at a time.** `tz` has no non-local locks. Claims stop
   accidents. They are not a way to coordinate several agents running at once.
3. **Claim before starting, close when done.**
4. **If it's going to be committed, it gets a ticket.** This includes small
   one-off requests (see [One-off tasks](#one-off-tasks)).
5. **Capture too much rather than too little.** Anything outstanding, out of
   scope, or unverified goes into triage. Don't leave it in a close reason, a
   chat message or a code comment, where no process will review it.
6. **Agents don't push, and don't create branches unless told to.**
7. **Agents that find things don't plan them.** Only the planning skills
   (`/spec`, `/to-tickets`, `/triage`, `/issue-hygiene`) shape the backlog.

## Concepts

### Role vs assignee

`role` (`human` or `agent`) says **who the work is for**. It is the main
dividing line in this workflow:

- Any open, unblocked work with `role: agent` can be picked up by an agent.
- `role: human` work is your queue: `tz ready -r human`.
- An agent task can wait on you: create a `role: human` task, then make the
  agent task `blocked_by` it.

`assignee` is separate and loose. Here it records **who actually took the
work**. `tz claim` sets it. For agents it is `<model>/<session>` (see
[`/build`](#build)). It is for tracking and auditing, not routing.

### Issue types

| Type | Used for | Appears in `tz ready` | `role` |
|------|----------|-----------------------|--------|
| `task`, `bug`, `feature`, `chore` | Work | yes | required |
| `gate` | A placeholder blocker with no work attached. Closed when an outside condition becomes true (upstream release, event, decision). | yes | required: who checks the condition |
| `epic` | Groups related work. Children link to it with `child_of`. | no | forbidden: an epic belongs to the owners of its children |
| `spec` | The output of planning (see [Specs](#specs)) | no | forbidden: a spec belongs to the planning session with the human |

The rule, enforced by `tz`: `role` is required exactly for the types `tz ready`
can return (work types and `gate`) and forbidden on `epic` and `spec`. Epics
and specs are never picked up as work, so a role on them would only mislead
(an `agent` spec would suggest an agent may pick it up). `tz create`,
`tz triage promote` and `tz reopen` follow the rule; `tz update -r` fails on an
epic or spec, and `tz update --clear-role` fails on the other types. Use
`tz list -r none` to find issues with no role. An epic or spec that still has a
role from before the rule loads normally; only setting a role on it fails, and
`tz update --clear-role` removes it.

Body conventions by type:

- **Work types:** Description (the context a fresh agent needs), Design (when
  an approach has been decided), Acceptance Criteria (always), Notes.
- **Epic:** Description and Acceptance Criteria. By default the criteria are
  "all children closed". Add more only if they can't be expressed as a child
  task.
- **Spec:** a free-form body in the Description section. It can be long, with
  numbered sections that tickets refer to as `<spec-id> §N`.
- **Gate:** Description says the condition (what has to happen) and how to
  check it.

`tz` has one body format for all types; per-type templates and checks are
still open (see [Open items](#open-items)).

### Triage: the inbox

A triage item (`tz triage add`) is plain text. It has no priority, role,
assignee or dependencies, and that's deliberate. Whoever finds something
(builder, reviewer, validator, the human) writes it down and goes back to what
they were doing. Deciding whether, how and when to deal with it belongs to
[`/triage`](#triage).

A good item says **what** was seen, **where**, **how to reproduce it** (if it
applies), and **which issue it was found during** (if any). If it's a request
for the human, say so plainly, along with what it blocks.

### Dependencies used here

| Type | Meaning in this workflow |
|------|--------------------------|
| `blocked_by` | Not ready until the target is closed. The only link that controls order. |
| `child_of` | Task → its epic. |
| `tracks` | Epic → the spec it implements. |
| `related_to` | Task → spec, when there is no epic. Amendment spec → original spec. |
| `discovered_from` | New issue → the issue it was found while working on. |

Blockers are inherited down `child_of`: an issue is not ready, and `tz blocked`
reports it, while any ancestor epic has an open `blocked_by` target or is in
`blocked` status. Block the epic once; its children (and their children) wait
with it. Don't copy an epic's blockers onto its children.

### Status

| Status | How it's used |
|--------|---------------|
| `open` | The backlog. |
| `in_progress` | Claimed. Only set by `tz claim`. |
| `closed` | Done, or no longer relevant (with a reason). |
| `blocked` | Rare. A short-term marker for "stuck, and the fix isn't a ticket yet". The normal mechanism is a `blocked_by` link to a real issue. |
| `deferred` | Only when the human decides. Agents may *suggest* deferring, never do it. |

### Priority and effort

Priority is `0`–`4`, where `0` is highest. `tz ready` sorts by priority, then
by age. Effort is `1`–`5`. The planning skills set both. Exact scales for each,
and using effort to choose a model or thinking level, are left for later.

## Flows

### One-off tasks

The human asks for something small in chat ("fix X"). There is no spec and no
`/to-tickets`. If the result will be committed, the agent:

1. creates a ticket (`tz create ... -r agent`) with a short description and
   acceptance criteria,
2. claims it, does the work, closes it,
3. commits with the ticket ID (as in [`/build`](#build)).

If nothing will be committed (a question, an investigation, a throwaway
script), no ticket is needed.

### A planned cycle

```
conversation ──/spec──▶ spec ticket ──/to-tickets──▶ epics + tasks
                                                     │
          ┌──────────────────────────────────────────┘
          ▼
   /build (one ticket per session) ──finds──▶ triage items
          ▲                                       │
          └────────── /triage, /issue-hygiene ◀───┘
```

Every arrow is optional. `/to-tickets` can work straight from a conversation
or a document. `/build` works on any ticket, however it was created. `/triage`
runs whenever the human asks for it.

### Specs

A spec keeps the results of planning in the tracker instead of in `docs/`. It
can be closed, linked to the work it produced, and still looked up later.

- **Open:** a draft that can be edited freely.
- **Closed:** once `/to-tickets` has turned it into tickets. From then on it
  doesn't change. It records what was decided and why. Epics link to it with
  `tracks`.
- **Changing a closed spec:**
  - *Small or medium change:* write an **amendment spec** titled
    `Amend <id>: ...`, linked to the original with `related_to`. It describes
    only what changes. The epic `tracks` both. Running `/to-tickets` on the
    amendment edits, closes or adds tickets.
  - *Change bigger than the original:* write a **replacement spec**. Close the
    old one with `--reason "superseded by <id>"`.
  - Don't edit a closed spec in place. It would stop matching the tickets and
    reasoning built from it.
- **Content needed after the work is done** (how things work, how to use them)
  becomes real docs during implementation. The spec is not the documentation.
- Not every piece of work needs a spec. Write one when there is design
  reasoning worth keeping.

### Asking the human for something

When an agent needs something from the human that can't easily be settled in
chat (a decision, credentials, a manual check, an outside action):

- **Planning skills** create a `role: human` task directly. They link every
  issue it blocks with `blocked_by`. The task says what is needed and where to
  record the answer, by default the task's Notes.
- **Builders** that hit this mid-ticket follow [Getting stuck](#getting-stuck).
  For non-blocking requests, they file a triage item marked as a human
  deliverable that names what it affects. `/triage` turns it into a task.
- **The human** does the task, records the answer, and closes it, either with
  `tz close` or by asking an agent to. The blocked agent work then becomes
  ready. The builder that picks it up reads its closed blockers.

### Getting stuck

If a builder can't meet its ticket's acceptance criteria (it's blocked, it's
missing information, or a criterion is wrong):

- it **does not close** the ticket,
- it leaves the ticket claimed (`in_progress`) and **tells the human in
  chat**,
- the human decides: answer in chat and let it continue, have a human task and
  blocker recorded (and the claim released), rewrite the ticket, or something
  else.

Builders don't create full issues and don't move on to other tickets unless
told to.

### Done, with loose ends

If the acceptance criteria are met but something is left over (part of the
work couldn't be checked, an edge case is out of scope, a nearby bug was
spotted), the builder **closes** the ticket and **files a triage item** for
each loose end, naming the ticket ID.

### Follow-up work on a closed ticket

Create a **new** ticket linked to the old one with `discovered_from`. Reopen
only when the close was a mistake (wrong ID, work reverted). A closed ticket
belongs to its commit. Reopening it would mix two rounds of work, and its
history would still say "done".

## Skills

Short specs. Each skill's `SKILL.md` adds the exact wording, examples and
commands.

All planning skills **show a plan and wait for approval** before writing,
except where noted. None of them commit unless told to.

### `/spec`

**Purpose:** turn a planning conversation or a provided document into a spec ticket.

**Input:** the passed input or document, then a conversation with the human, plus whatever research is needed (code, docs, the existing backlog).

**Steps:**
1. Work through the problem with the human: ask questions, push back on the
   approach, settle what's in and out of scope.
2. Write the spec: problem, goals and non-goals, design, open questions. Use
   numbered sections if tickets will need to refer to parts of it.
3. Create it: `tz create "<title>" -t spec --description-file -`.
4. If something has to come from the human and can't be settled now, create a
   `role: human` task and make the spec `blocked_by` it.

**Rules:**
- Doesn't create epics or work tickets, and doesn't name epics. That is
  `/to-tickets`' job.
- Leaves the spec open.
- Changes to a closed spec become an amendment or a replacement (see
  [Specs](#specs)).

### `/to-tickets`

**Purpose:** turn a spec into concrete, buildable `tz` issues.

**Input:** a spec ticket ID, a document, or the current conversation. If the
input contains design reasoning worth keeping and isn't a spec ticket yet,
offer to run `/spec` first.

**Steps:**
1. Read the input and enough of the code to make tickets concrete. Check open
   issues and triage for overlap.
2. Draft:
   - **Epics**, where a group of tasks forms one deliverable.
   - **Work tickets**, each with type, title, role, priority, effort,
     description, acceptance criteria, and design notes where an approach has
     been decided.
   - **`blocked_by` links** between tickets.
   - **Human tasks and gates** where needed.
   - Any **open blockers on the spec** carried over to the tickets that depend
     on the answer.
3. Show the plan, then revise it with the human.
4. Create epics first, then tickets (`--parent`, `--deps`). Link each epic to
   the spec (`tz dep add <epic> --target <spec> --type tracks`). Without an
   epic, link the tickets with `related_to`.
5. Close the spec with `--reason "ticketed: <epic or ticket ids>"`.

**Ticket quality:**
- Every work ticket has testable acceptance criteria.
- Every ticket makes sense to an agent with no conversation history. Refer to
  spec sections (`<spec-id> §N`) instead of repeating them.
- Every ticket fits in one `/build` session.

### `/build`

**Purpose:** implement **one** ticket, then stop.

**Input:** none, or a ticket ID chosen by the human.

**Steps:**
1. Choose: the given ticket, otherwise the first result of `tz ready -r agent`
   (priority, then age). Results are open, unblocked work. Epics and specs are
   excluded.
2. Read: `tz show <id>`, its closed blockers (answers from the human are
   there), its epic, and the spec the epic tracks.
3. Claim: `tz claim <id> --as <model> --team <session>`, e.g.
   `--as "$PI_MODEL" --team "${PI_SESSION_ID:0:8}"`. That gives
   `assignee: claude-opus-5-5/01a1c3e8`.
4. Implement. Run the checks the project's `AGENTS.md` asks for.
5. Capture: anything non-blocking and out of scope found along the way goes
   into `tz triage add`.
6. Code review: When done, use the `/code-review` skill for your work, which will dispatch sugagents. Address any feedback reported by them.
6. Finish:
   - **Acceptance criteria met:** `tz close <id>` with no reason (the commit is
     the record). Loose ends go to triage (see
     [Done, with loose ends](#done-with-loose-ends)).
   - **Not met:** see [Getting stuck](#getting-stuck). Don't close.
7. Commit (on the current branch). Unless the `no-commit` option is invoked, commit your work. Stage only this session's changes: code,
   the ticket file, and any triage items it created. Leave other uncommitted
   changes alone. The message starts with the ticket ID.
8. Stop and report: what changed, the commit, and any triage items filed.

**Close reasons** are only for unusual cases: the ticket no longer applies,
it was fixed in another session, or the work spans several commits.

**Variant, no commit (when the human says so):** same steps, but stop after
closing. The close sits in the working tree with the code changes. Committing
records both, and discarding the changes also undoes the close.

**Rules:** one ticket per session. Don't create full issues, don't push, no
branches unless told. Choosing a model or effort level by ticket effort may
come later.

### `/triage`

**Purpose:** process the triage inbox. It is a planner: it doesn't implement.

**Steps:**
1. Read the inbox (`tz triage list --json`) and the open backlog.
2. Assess each item. Is it valid (check the code)? Is it a duplicate (of
   another item or an existing issue)? Is it already fixed? Is it actually a
   human request?
3. Plan, per item or group of items:
   - **promote** to a new issue, combining related items, with role,
     priority, effort, type, acceptance criteria, an epic (new or existing),
     `blocked_by` links, and `discovered_from` the issue it came from;
   - **attach** to an existing issue as a comment;
   - **remove** (invalid, duplicate, already fixed), with the reason in the
     plan;
   - **human task** with `blocked_by` links on the issues it blocks.

   Also plan any **edits to existing issues** (priority, blockers, parent) and
   list **deferral candidates** for the human to decide on.
4. Show the plan and revise it with the human.
5. Carry it out with `tz triage promote` / `attach` / `rm` and `tz update` /
   `tz dep add`. Rewrite item text into proper descriptions as needed.
6. Summarize what was done.

**Rules:** don't defer anything without approval. For follow-up work, prefer a
new ticket over reopening one. Every promoted work issue has acceptance
criteria.

### `/issue-hygiene`

**Purpose:** keep the backlog consistent while the workflow is still being
tuned. It may grow or go away.

**Done without asking, then reported:**
- Close epics whose children are all closed and whose acceptance criteria say
  nothing more (`--reason "all children closed"`).

**Planned first, then done once approved:**
- Release claims (`tz claim --release --force`) on `in_progress` issues that
  nothing is working on. A stuck builder leaves its claim on purpose, so check
  with the human.
- Close epics whose children are all closed but whose acceptance criteria go
  further.

**Only reported:**
- `blocked`-status issues whose reason may no longer apply.
- `blocked_by` links to issues that don't exist.

## Quick reference

| Who | Command | Purpose |
|-----|---------|---------|
| anyone | `tz triage add "..."` (or pipe text) | Capture a finding |
| human | `tz ready -r human` | Your queue |
| human | `tz tui` | Browse issues and triage |
| builder | `tz ready -r agent` | Agent queue |
| builder | `tz claim <id> --as <model> --team <session>` | Start |
| builder | `tz close <id>` | Finish (commit follows) |
| planner | `tz create ... -t spec\|epic\|task [-r ...] --parent ... --deps ...` (no `-r` for spec and epic) | Shape work |
| planner | `tz dep add <epic> --target <spec> --type tracks` | Link an epic to its spec |
| planner | `tz triage promote\|attach\|rm` | Clear the inbox |

## Open items

- Per-type body templates and checks (triage).
- Scales for priority and effort; choosing model and thinking level by effort.
- Reviewer and validator subagents before `/build` closes a ticket.
- Comment conventions.
