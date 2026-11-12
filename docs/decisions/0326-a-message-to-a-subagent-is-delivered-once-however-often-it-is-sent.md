---
status: proposed
---

# A program sends a message to a subagent, delivered once however often it is sent

## Problem

A program wakes a subagent when a reminder it set comes due, or when an
event the subagent waits for happens. The web API reaches only a
conversation's main agent, with the messages and steers of the page
connection's conversation lane. A program can read a subagent's transcript
(`GET /conversations/{id}/agents/{node}/transcript`) but cannot write to
it, and the agents' own `demi agent send` reaches only a live agent, while
an ended one is resumed only by its parent.

The program also retries: when the connection drops before an answer, it
sends again, and the subagent must work once.

## Decision

`POST /conversations/{id}/agents/{node}/messages` sends a message to the
subagent `node`, the id the conversation's agents list and the transcript
route take:

```rust
/// A message to one subagent.
pub struct SubagentMessage {
    /// Its id, which a retry repeats so nothing is sent twice; one is made
    /// when none.
    pub id: Option<BlockId>,
    /// Not blank, as a message to the main agent is not.
    pub text: String,
}
```

It answers `200 { id }`, so a program that named no id learns the one made
for it. What a call does depends on the subagent and on what it holds:

| The subagent | It holds the id | What the call does |
| --- | --- | --- |
| Live | In its history or queue | Nothing; answers the id. |
| Live | No | Hands it the message: a running turn takes it as a steer, and an idle subagent starts a turn with it. |
| Ended | In its history | Nothing; the subagent stays ended; answers the id. |
| Ended | No | Resumes it with the message, under that id, as its parent's `demi agent resume` would. |
| Stopping | Any | Refuses, 409: the same call once it has stopped finds it ended. |
| Ended, under a subagent that ended | No | Refuses, 409: only a running parent resumes its subagent, so the program messages the parent first. |

- An id belongs to one agent: the same id sent to two subagents is two
  messages. A repeated id with other text is the same message, and the
  first text stands, as with a repeated steer.
- A message that a stop left unread is in no history; a retry of it resumes
  the subagent, which then reads it once.
- A message to a subagent whose command waits for a permission denies the
  request, as a message to the main agent denies the main agent's: the
  command ends, and the subagent reads the message next.
- The main agent's node answers 404, as a node the conversation does not
  have does, saying that its messages go to the conversation.
- The conversation refuses the message as it refuses a page's open, with
  that refusal's code and 409: an archived conversation, one with no model,
  or one whose provider entry is gone.
- A 409 refusal answers the same call again until what refused it changes.
  A 500 is a failure inside the core, and a 503 a core that is closing; the
  same call may succeed later.
- The SDK's `Conversation` gains `sendToAgent(node, text, id?)`, which
  answers the id.

### Build

Nothing new is stored. The history holds each id already: a message that
opens a turn as its user block's turn, and a steer that joins a turn as its
block's id.

- **A live subagent.** Its session accepts a steer whose id it holds once,
  as it does for the main agent (`crates/agent-session/src/core.rs`), so a
  repeated id joins nothing.
- **An ended subagent.** The transcript's reader answers whether its history
  holds the id.
- **Two equal calls at once.** The message to an ended subagent starts it in
  the tree's queue of starts, one start of a parent's children at a time,
  and each check runs in the queue: the second call finds the subagent live
  and holding the id in its queue, or, if it ended meanwhile, finds the id
  in its history.
- **A resume under an id.** The reopened subagent takes the queued message
  whole, so it keeps the program's id; `demi agent resume` makes an id, as
  today.

A core that closes between saving the resume and answering restores the
subagent with the message in its queue, so the retry finds it.

## Spike

None.

## Prior art

- [OpenCode's server](https://opencode.ai/docs/server/) takes an optional
  `messageID` on `POST /session/:id/message`.
- [Stripe](https://docs.stripe.com/api/idempotent_requests) takes an
  `Idempotency-Key` and saves the first response for retries. Demi saves no
  response: the answer is only the id, which the retry holds, and the
  history is the record of what arrived.

## Alternatives

- **A table of the ids seen.** It would store again what the history holds,
  and need an expiry of its own.
- **An `Idempotency-Key` header.** The main agent's messages name the same
  thing `id` in their body, and one concept has one name.
- **Only live subagents.** A retry after the subagent ended would resume it
  and run the work again, which is the case this decision exists for.

## Open questions

None.
