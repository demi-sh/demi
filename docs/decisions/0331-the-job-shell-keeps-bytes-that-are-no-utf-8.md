---
status: accepted
---

# The job shell keeps bytes that are no UTF-8

## Problem

A job reads a file that is not UTF-8, such as a Latin-1 export, and passes
it on:

```bash
name=$(cat latin1.txt)     # the bytes 63 61 66 e9, "café" in Latin-1
printf '%s\n' "$name" > copy.txt
```

GNU bash writes the four bytes it read. Jush's job shell (brush, with
uutils in the same process) holds every value as a Rust `String`, so the
byte `e9` became U+FFFD where the text entered the shell, and `copy.txt`
holds `63 61 66 ef bf bd`. The same happens to a file name that is no
UTF-8, which a glob, `find` or `xargs` then names wrongly or not at all,
to `$'\xff'`, `read`, `mapfile`, `echo -e` and `printf -v`. The damage is
silent: the job succeeds and the data is changed.

## Decision

The shell's text keeps every byte it read. A byte that is no UTF-8, always
one of 0x80 to 0xFF, is held as one character, U+10FF00 plus the byte, the
last 128 code points of the private-use plane 16, and is written as that
byte wherever text leaves the shell. Within the shell it is one character,
as bash counts it.

Those 128 characters stand for nothing else. One that arrives as UTF-8,
such as a file name holding U+10FFFF, is held as its four bytes, so it
comes out as it went in. No ASCII byte is ever held, so no text can turn
into a `/`, a NUL or a quote on its way out.

```text
  bytes in ── decode ──> "caf\u{10FFE9}" ── encode ──> bytes out
  63 61 66 e9                                         63 61 66 e9
```

One module of the shell, `brush-core/src/bytes.rs`, owns the scheme:

```rust
pub fn byte_char(byte: u8) -> char;
pub fn char_byte(c: char) -> Option<u8>;
pub fn decode(bytes: &[u8]) -> String;          // and decode_cow
pub fn encode(text: &str) -> Cow<'_, [u8]>;
pub fn normalize(text: &str) -> Cow<'_, str>;   // joined parts made whole
pub fn append(base: &mut String, suffix: &str); // `+=`
pub fn lossy(text: &str) -> Cow<'_, str>;       // for a UTF-8 protocol
pub fn byte_len(text: &str) -> usize;           // what C locale counts
pub fn from_os(name: &OsStr) -> Cow<'_, str>;
pub fn to_os(text: &str) -> Cow<'_, OsStr>;
pub fn path(path: &Path) -> Cow<'_, Path>;
pub struct ByteWriter<W>;                       // a builtin's output
```

- **Where text enters:** command substitution, `read`, `mapfile`, a
  script run, sourced or read from standard input, `$'…'`, `echo -e`,
  `printf -v`, `${v@E}`, the names a glob lists, the working directory, a
  path the shell found or printed (`pwd`, `dirs`, `command -v`, `type`,
  `hash`), the environment the runner starts with, and the arguments and
  environment a utility hands a nested shell, `timeout` or `nohup` all
  decode.
- **Where text leaves:** a builtin's standard output and error, and the
  trace of `set -x`, go through `ByteWriter` or `encode`; a program's
  path, arguments and environment through `to_os`; every path the shell
  opens, tests or changes into through `Shell::absolute_path`, and every
  directory of `PATH` it searches, through `path`; a here-document
  through `encode`. `printf` formats the bytes its format and arguments
  stand for and writes them to the stream itself. `%q` and `${v@Q}` quote
  a held byte as `$'\377'`, as bash does.
- **Joining:** `$a$b` and `a+=$b` make held bytes that meet whole again
  (`normalize`), so `a=$'\xe4'; b=$'\xb8\xad'; c=$a$b` is the one
  character `中`, as in bash.
- **Counting:** in a UTF-8 locale a held byte is one character; outside
  one, `${#v}` and `${v:i:n}` count and cut the bytes `encode` gives.
- **The utilities in the shell's process** take the bytes themselves:
  uucore's invocation holds its environment as `OsString`s; `find`'s
  starting points, `-print`, `-print0` and `-printf`, `xargs`'s command
  and arguments, and `which` keep the bytes instead of replacing them with
  U+FFFD.
- **A protocol of UTF-8 strings,** such as a declared `jush` command's
  arguments and environment, gets U+FFFD for a held byte (`lossy`).
- **Windows** holds no bytes in its strings: `to_os` and `path` there give
  U+FFFD for a held byte, as before.

What stays as text: `find`'s expression (`-name`, `-path`, `-regex`) and
`sed`'s script read a byte that is no UTF-8 as U+FFFD, as before, since
both parse their arguments as `&str`; GNU's take the bytes.

## Spike

None: the GNU behavior each case must match was read from bash 5.2, GNU
sed 4.9 and GNU findutils on Linux, and every case was compared with it.

## Prior art

- [fish](https://github.com/fish-shell/fish-shell/blob/master/crates/widestring/src/lib.rs)
  holds its text as wide characters and does the same with
  `ENCODE_DIRECT_BASE`, U+F600 to U+F6FF in the Basic Multilingual Plane's
  private-use area; a character that arrives in that range is held as its
  bytes too ([`str2wcs_internal`](https://github.com/fish-shell/fish-shell/blob/3.3.1/src/common.cpp)),
  as Jush does. Jush takes plane 16 instead, which no common font draws
  from, where the Basic Multilingual Plane's area holds icon fonts' glyphs
  that a prompt or a file name may contain.
- Python's [PEP 383](https://peps.python.org/pep-0383/) `surrogateescape`
  maps each such byte to a lone surrogate, U+DC80 to U+DCFF. A Rust
  `char` cannot hold a lone surrogate, so Jush cannot take that range.
- bash and other C shells hold bytes and need no scheme.

## Alternatives

- **Values as bytes (`Vec<u8>` or `OsString`) throughout brush.** It is
  what bash does, but brush's expansion, patterns, regular expressions and
  every builtin take `&str`: the change touches nearly every file of the
  fork and departs from upstream everywhere.
- **Keep U+FFFD.** Simple, and it changes data silently.

## Open questions

None.
