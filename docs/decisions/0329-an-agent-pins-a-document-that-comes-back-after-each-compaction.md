---
status: accepted
---

# An agent pins a document that comes back after each compaction

## Problem

Alex asks the agent to follow `docs/release-checklist.md` for every release
of the day. The agent reads it and follows it. Three hours in, the
conversation is compacted: the summary says *follow the release checklist*,
and the checklist's twelve steps are gone from what the model reads. The next
release skips the step that tags the build. The agent could read the file
again, but nothing tells it to, and Alex only finds out from the missing tag.

A rule that should hold for the whole conversation has no place to live in
it. `AGENTS.md` holds rules for every conversation in a project, and the
summary keeps what a rule means, not its words.

## Decision

The agent pins a document; after every compaction, the model reads the
document's current text again, right after the summary. The composer shows
each pinned document, and the user can unpin it.

```text
  ┌───────────────────────────┐
  │ 📌 release-checklist.md × │   ← the chips row over the composer
  └───────────────────────────┘
  ┌──────────────────────────────────────────────────────────────┐
  │ Ask anything                                                 │
  │ (+)  Workflow                            Opus 5.5  ◔   (↑)   │
  └──────────────────────────────────────────────────────────────┘
```

* **The agent pins.** `jush pin add <path>` pins a text file of the device
  the command runs on. `jush pin remove <path>` unpins it, and `jush pin
  list` lists what is pinned. Only the main agent pins: the composer shows
  its pins, and the user controls only what the user sees. In a subagent,
  `jush pin add` and `jush pin remove` fail and say so.
* **A context source gives the text.** The pinned documents are a context
  source, as the execution context is. Before each request of the main
  agent, it compares the pins with what its blocks since the last compaction
  show the agent, and answers what the agent lacks as one context block.
  Each block tags a document's text with its pin's id and an unpin it tells
  with `(pin N)`. Right after a pin, that is the new document's text. After
  a compaction it is every pinned document whose text the part of the
  transcript the compaction kept does not show. Nothing is added to the
  system prompt, so pinning and unpinning leave the prompt cache as it was,
  and the transcript holds each block, so a later request replays the same
  bytes.
* **The current text.** Each block reads the documents from their devices
  then, without waking a device. A document that cannot be read, its device
  offline or the file gone, comes as last read, with a line that says so.
* **What the model reads**, as a context block:

  ```text
  [Pinned documents]
  Pinned documents, as they are now. Keep following them.

  <pinned id="3" path="/Users/alex/atlas/docs/release-checklist.md" device="MacBook Pro">
  …
  </pinned>
  ```

* **The user unpins.** The chip's × unpins the document. It does not wake the
  agent or stop its turn: its next request carries the change, *The user
  unpinned /Users/alex/atlas/docs/release-checklist.md on MacBook Pro (pin
  3): stop following it, and do not pin it again unless the user asks.* The
  model would otherwise keep following a document still in its context and
  might pin it again. An unpin that comes before a compaction is told after
  it. The agent's own unpin needs no word.
* **The chip.** It stands in the chips row over the composer, where Continue
  and Resume stand: the pin's mark and the file's name, its path and device
  in the tooltip, and a ghost × as the comments' pill has. A click opens a
  file of the conversation's device, as a file link in a reply does; a file
  of another device only shows. Pins appear and leave as the agent's command
  or the user's × changes them, on every page that shows the conversation.
* **A limit.** The pinned documents together hold at most 25,000 tokens:
  each comes back after every compaction, and a compaction has to leave the
  model most of its window. `jush pin add` refuses a document that would pass
  it, and names how much is pinned. Documents that grew past the limit by the
  next compaction come back cut, with a line to read the rest from the file.
  The limit bounds the block, so replay sends it whole, as it sends the
  instructions, where it keeps only the start and the end of any other long
  context. A file is pinned once at a time.

The control database holds the pins, their one owner: each pin's id, which
counts within its conversation, its device, absolute path, when it was pinned
and unpinned, by whom, whether the agent was told of the user's unpin, and
its text as last read. A row stays once unpinned, and its id is never taken
again. A Fork copies its source's rows with their ids, which the transcript it
copies names, so its first request gives only what the copied transcript
lacks. The conversation's metadata lists the pinned documents, and the socket
sends it again at each change. An older page shows no chips and offers no ×;
pins still come back.

## Spike

Claude Code 2.1.296's own bundle, read for its constants and texts: after a
compaction it puts back the plan file read again from disk, the last
invocation of each skill used (at most 5,000 tokens each, 25,000 together),
and up to five files read recently (5,000 tokens each, 50,000 together). A
skill comes with *shown here for context only so you remain aware of their
guidelines*. Whether a model keeps following a pinned document after a real
compaction is not measured yet; the product check runs one.

## Prior art

* **Claude Code** puts back what it judges the session still needs, as above.
  Neither the user nor the model chooses the files, and nothing shows what
  came back. Here the agent chooses, the composer shows it, and the user can
  take it away.
* **aider**'s `/read-only` adds a file the user names to every request. Here
  the agent pins, and the document is put back only after a compaction.
* **OpenCode** and **Codex** have open requests for the same thing: files
  pinned across compaction, with a limit (OpenCode), and a
  `context_pinned_files` option (Codex `openai/codex#16839`).

## Alternatives

* **The system prompt.** Every pin, unpin and edit of a document would change
  the prompt and lose the whole cache.
* **Every request.** The document would be in context twice until a
  compaction, and paid for in every request.
* **The compaction's block.** The compaction could keep the documents' text
  beside its summary. It reads them only when it runs, and the agent would
  still need the text when it pins and word of the user's unpin, which a
  context source gives the same way.
* **The user pins too.** Asked and refused: the agent decides what its work
  needs.
* **Ask the summary to keep it.** A summary keeps what a rule means, and a
  long one loses the steps first.

## Open questions

None. *Pin* is also what the sidebar does to a conversation and the taskbar
to a window; all three keep a thing at hand.
