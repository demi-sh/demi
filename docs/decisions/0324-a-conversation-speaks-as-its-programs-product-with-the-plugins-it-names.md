---
status: proposed
---

# A conversation speaks as its program's product, with the plugins it names

## Problem

A program runs a help desk's assistant on Demi. Every agent's system prompt
opens with "You are Demi, an agent that works for the user on their
computers: a Demi Cloud machine or the devices they paired", and the guide
after it says that Files shows the working directory. The assistant is
neither: it answers a team's questions and has no Files view. The program's
own instructions can only follow that sentence, so the model reads two
identities.

The program also declares two plugins: `tickets`, and `tickets-next`, the
next version of the same commands, which it tries in one conversation
before the others. Every conversation has both, so each agent reads two
sets of the same commands and may run either.

## Decision

`CreateConversation` and `PATCH /conversations/{id}` take two more fields:

```rust
pub struct ConversationPatch {
    // ...
    /// The sentences that open its agents' system prompts in place of
    /// Demi's, such as "You are the help desk's assistant."; null for
    /// Demi's.
    pub identity: Option<Option<String>>,
    /// The program's plugins its agents have, by id; null for all of them.
    pub plugins: Option<Option<Vec<String>>>,
}
```

The conversation's metadata names both, as it names the model.

### The identity

- It replaces `INSTRUCTIONS` (`crates/core/src/conversation/product.rs`)
  wherever an agent of the conversation reads it today, a subagent's
  included. Text of white space alone is refused, as it says nothing.
- The guide after it keeps the runtime's rules, since an agent cannot use
  Demi's commands without them. For a conversation with an identity, its
  first rule ends at "The directory stays between turns", without "and
  Files shows it", a convention of the web app's display.
- A change reaches each live agent of the conversation from its next
  request, as a change of the user's instructions does; that request misses
  the provider's prompt cache once. A Claude Code process, whose CLI reads
  the system prompt once as it starts, is not continued for a request with
  another prompt: a new one resumes the session.

### The plugins

- The list names only the program's plugins, the ones a program declares
  (`PUT /plugins/{id}`, 0327). Demi's own plugins follow the user's
  switches, as today, and a conversation of the web app names none.
- An id the process has no plugin of gives nothing, and is not refused: a
  conversation may name a plugin that its program declares later.
- The conversation's toolset holds the plugins it names, and its revision
  names them, so two conversations that name different plugins never share
  a toolset.
- A program's plugin the conversation does not name gives it no context
  block either.
- A change reaches each live agent's commands from its next command, and
  the list of commands in its system prompt from its next request, as a
  change of the identity does. A plugin's new declaration still changes
  every live conversation's commands at once and leaves the list in their
  prompts until each next opens (0008), so one declaration does not miss
  every conversation's cache.

### Build

- The conversation's record in `core.sqlite` keeps both, in two columns,
  null for a conversation of the web app.
- The agent server takes the identity and the guide from a source per
  conversation, read as its tree opens and after a change, instead of the
  two strings it gets once at its start (`conversation/mod.rs`). The
  source fails the tree's open when it cannot read the record, rather than
  open it as Demi.
- A user's switch or a program's declaration gives each live tree its
  toolset under the tree's opening order, as a change of the conversation's
  list does, so the later of the two holds what the record says last.
- `CoreToolsets` composes the conversation's toolset: the plugins the user
  has on, less the program's plugins the conversation does not name.

## Spike

None.

## Prior art

- OpenAI's Responses API takes `instructions` from the caller, with
  nothing of the platform's before them.
- Claude Code's `--system-prompt` replaces its default prompt, while
  `--append-system-prompt` adds to it; the Claude Agent SDK takes
  `systemPrompt` and the tools a query has.
- OpenCode's agents each name the tools they have.

Here the identity is replaced and the runtime's rules stay, and the
selection is by plugin, the unit a program declares and a user switches.

## Alternatives

- **The identity in the user's instructions.** They follow the identity;
  they cannot remove it.
- **A selection by command root.** A plugin may declare several roots, and
  the program declares, routes and replaces whole plugins.

## Open questions

None.
