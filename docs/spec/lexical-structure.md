# The nudl Programming Language Specification

Version 0.1.0 — Draft

> The power and syntax of Rust, with the memory management of Swift and the
> metaprogramming of Zig.

---

## 1. Introduction and Notation

### 1.1 Purpose

This document is the normative specification of the nudl programming language. It
defines the lexical structure, type system, memory model, expression and statement
semantics, module system, and compile-time evaluation model. Implementations of
nudl must conform to the rules described herein. Where this document is silent,
behavior is implementation-defined.

### 1.2 Notation Conventions

Grammar rules are given in an EBNF-like notation:

```
non_terminal  = expression ;
```

The following meta-syntax is used throughout:

| Notation      | Meaning                                         |
|---------------|-------------------------------------------------|
| `'text'`      | Terminal string (literal token)                 |
| `A B`         | Sequence: A followed by B                       |
| `A \| B`      | Alternation: A or B                             |
| `A?`          | Optional: zero or one occurrence of A           |
| `A*`          | Repetition: zero or more occurrences of A       |
| `A+`          | Repetition: one or more occurrences of A        |
| `( A )`       | Grouping                                        |
| `A % ','`     | Comma-separated list: `A ( ',' A )* ','?`       |
| `/* text */`  | Informal prose describing a rule                |

When a grammar rule is too complex to express purely in EBNF, informal prose in
square brackets supplements the rule.

### 1.3 Terminology

| Term              | Definition                                                        |
|-------------------|-------------------------------------------------------------------|
| **binding**       | An association between a name and a value, introduced by `let`.   |
| **value type**    | A type whose instances are copied on assignment.                  |
| **reference type**| A type whose instances are heap-allocated and reference-counted.   |
| **item**          | A top-level declaration: function, struct, enum, interface, impl, type alias, import. |
| **expression**    | A construct that evaluates to a value.                            |
| **statement**     | A construct executed for its side effects; may contain expressions.|
| **pattern**       | A construct that destructures a value and optionally binds names. |
| **comptime**      | Compile-time evaluation context.                                  |
| **ARC**           | Automatic Reference Counting.                                     |
| **monomorphization** | The process of generating a specialized copy of a generic definition for each distinct set of type arguments. |

---

## 2. Lexical Structure

### 2.1 Source Encoding

A nudl source file is a sequence of bytes encoded in UTF-8. A byte order mark
(U+FEFF) at the beginning of a file is ignored. Implementations shall reject
source files containing invalid UTF-8 sequences.

The canonical file extension is `.nudl`.

### 2.2 Whitespace

The following characters are whitespace and are ignored except as token separators:

- U+0020 SPACE
- U+0009 HORIZONTAL TAB
- U+000A LINE FEED
- U+000D CARRIAGE RETURN

Whitespace is not significant to the grammar. Semicolons, not newlines, terminate
statements.

### 2.3 Comments

nudl supports two forms of comments:

```
line_comment   = '//' ( /* any character except LINE FEED */ )* ;
block_comment  = '/*' ( block_comment | /* any character */ )* '*/' ;
```

Block comments nest. The sequence `/* /* */ */` is a single valid comment.

Comments are treated as whitespace by the lexer. They do not appear in the token
stream.

### 2.4 Keywords

The following identifiers are reserved as keywords and cannot be used as user-defined
names:

```
actor     as        async     await     break     comptime
const     continue  defer     dyn       else      enum
extern    false     fn        for       if        impl
import    in        interface let       loop      match
mut       pub       quote     return    self      Self
struct    true      type      weak      where     while
```

| Keyword    | Description                                        |
|------------|----------------------------------------------------|
| `Self`     | The implementing type in interfaces and impl blocks |
| `extern`   | Foreign function declarations                      |
| `quote`    | Comptime code generation blocks                    |

### 2.5 Identifiers

```
identifier     = XID_Start XID_Continue*
               | '_' XID_Continue+ ;
```

Where `XID_Start` and `XID_Continue` are defined by Unicode Standard Annex #31.
In practice, ASCII identifiers follow the pattern `[a-zA-Z_][a-zA-Z0-9_]*`, but
the full Unicode set is accepted. The lone underscore `_` is a wildcard pattern,
not an identifier.

Identifiers are case-sensitive. `Foo`, `foo`, and `FOO` are distinct.

### 2.6 Literals

#### 2.6.1 Integer Literals

```
integer_literal  = decimal_literal
                 | hex_literal
                 | octal_literal
                 | binary_literal ;

decimal_literal  = DIGIT ( DIGIT | '_' )* integer_suffix? ;
hex_literal      = '0x' HEX_DIGIT ( HEX_DIGIT | '_' )* integer_suffix? ;
octal_literal    = '0o' OCTAL_DIGIT ( OCTAL_DIGIT | '_' )* integer_suffix? ;
binary_literal   = '0b' BIN_DIGIT ( BIN_DIGIT | '_' )* integer_suffix? ;

integer_suffix   = 'i8' | 'i16' | 'i32' | 'i64'
                 | 'u8' | 'u16' | 'u32' | 'u64' ;

DIGIT            = '0'..'9' ;
HEX_DIGIT        = '0'..'9' | 'a'..'f' | 'A'..'F' ;
OCTAL_DIGIT      = '0'..'7' ;
BIN_DIGIT        = '0' | '1' ;
```

Underscore separators may appear between digits for readability but are ignored.
Leading zeros in decimal literals are permitted (`007` is valid and equals `7`).

An unsuffixed integer literal has its type determined by type inference. If no
context constrains the type, it defaults to `i32`.

Examples:

```nudl
42              // i32 (default)
1_000_000       // i32, underscores for readability
0xFF_u8         // u8, value 255
0o77            // i32, value 63
0b1010_i64      // i64, value 10
```

#### 2.6.2 Float Literals

```
float_literal    = DIGIT ( DIGIT | '_' )* '.' DIGIT ( DIGIT | '_' )* exponent? float_suffix?
                 | DIGIT ( DIGIT | '_' )* exponent float_suffix?
                 | DIGIT ( DIGIT | '_' )* float_suffix ;

exponent         = ( 'e' | 'E' ) ( '+' | '-' )? DIGIT ( DIGIT | '_' )* ;
float_suffix     = 'f32' | 'f64' ;
```

A float literal must contain a decimal point, an exponent, or a float suffix to
be distinguished from an integer literal. An unsuffixed float literal defaults to
`f64`.

Examples:

```nudl
3.14            // f64 (default)
2.0e10          // f64, scientific notation
1.5f32          // f32
0.001_f64       // f64, explicit suffix
1e-3            // f64, no decimal point but has exponent
```

#### 2.6.3 String Literals

```
string_literal   = '"' string_char* '"' ;

string_char      = /* any character except '"', '\', LINE FEED */
                 | escape_sequence ;

escape_sequence  = '\n'               /* LINE FEED, U+000A        */
                 | '\r'               /* CARRIAGE RETURN, U+000D   */
                 | '\t'               /* HORIZONTAL TAB, U+0009    */
                 | '\\'               /* BACKSLASH, U+005C         */
                 | '\"'               /* DOUBLE QUOTE, U+0022      */
                 | '\''               /* SINGLE QUOTE, U+0027      */
                 | '\0'               /* NULL, U+0000              */
                 | '\x' HEX_DIGIT HEX_DIGIT
                                      /* arbitrary byte value      */
                 | '\u{' HEX_DIGIT+ '}'
                                      /* Unicode scalar value      */ ;
```

`\'` is valid in both string and character literals.

String literals produce values of type `string`. The content is UTF-8 encoded.
The `\x` escape produces a single byte; values above 0x7F must form valid UTF-8
when combined with surrounding characters, or a compile-time error is issued.
The `\u{...}` escape accepts one to six hexadecimal digits and must be a valid
Unicode scalar value (U+0000 to U+D7FF or U+E000 to U+10FFFF).

Examples:

```nudl
"hello, world"
"line one\nline two"
"tab\there"
"null byte: \0"
"hex: \x41"              // "A"
"unicode: \u{1F600}"     // grinning face emoji
```

#### 2.6.4 Template String Literals

```
template_string  = '`' template_part* '`' ;

template_part    = template_text
                 | '{' expression '}' ;

template_text    = ( template_char | escape_sequence | '\{' | '\}' | '\`' )+ ;

template_char    = /* any character except '`', '\', '{', '}' */ ;
```

Template strings are delimited by backticks. Expressions between `{` and `}` are
evaluated, converted to strings via the `Printable` interface, and concatenated
with the surrounding text. Literal braces are escaped as `\{` and `\}`. A literal
backtick is escaped as `` \` ``. Standard escape sequences (`\n`, `\t`, etc.) are
also supported.

The type of a template string literal is `string`.

Examples:

```nudl
let name = "world";
`hello, {name}`                    // "hello, world"
`1 + 1 = {1 + 1}`                 // "1 + 1 = 2"
`braces: \{ and \}`               // "braces: { and }"
`nested: {`inner {42}`}`          // "nested: inner 42"
```

#### 2.6.5 Character Literals

```
char_literal     = '\'' char_content '\'' ;

char_content     = /* any character except '\'', '\', LINE FEED */
                 | escape_sequence ;
```

A character literal represents a single Unicode scalar value. Its type is `char`.
The same escape sequences as string literals are supported.

Examples:

```nudl
'a'
'\n'
'\u{03B1}'      // Greek lowercase alpha
```

#### 2.6.6 Boolean Literals

```
bool_literal     = 'true' | 'false' ;
```

The type of a boolean literal is `bool`.

### 2.7 Operators and Punctuation

The following tokens are recognized as operators and punctuation:

**Arithmetic operators:**

| Token | Name                    |
|-------|-------------------------|
| `+`   | Addition                |
| `-`   | Subtraction / Negation  |
| `*`   | Multiplication          |
| `/`   | Division                |
| `%`   | Remainder               |

**Bitwise operators:**

| Token | Name                    |
|-------|-------------------------|
| `&`   | Bitwise AND             |
| `\|`  | Bitwise OR              |
| `^`   | Bitwise XOR             |
| `<<`  | Left shift              |
| `>>`  | Right shift             |

**Comparison operators:**

| Token | Name                    |
|-------|-------------------------|
| `==`  | Equal                   |
| `!=`  | Not equal               |
| `<`   | Less than               |
| `>`   | Greater than            |
| `<=`  | Less than or equal      |
| `>=`  | Greater than or equal   |

**Logical operators:**

| Token | Name                    |
|-------|-------------------------|
| `&&`  | Logical AND (short-circuit) |
| `\|\|` | Logical OR (short-circuit)  |
| `!`   | Logical NOT             |

**Assignment operators:**

| Token | Name                    |
|-------|-------------------------|
| `=`   | Assignment              |
| `+=`  | Add-assign              |
| `-=`  | Subtract-assign         |
| `*=`  | Multiply-assign         |
| `/=`  | Divide-assign           |
| `%=`  | Remainder-assign        |

**Range operators:**

| Token | Name                    |
|-------|-------------------------|
| `..`  | Exclusive range         |
| `..=` | Inclusive range         |

**Pipe operator:**

| Token | Name                    |
|-------|-------------------------|
| `\|>` | Pipe                    |

**Other operators:**

| Token | Name                    |
|-------|-------------------------|
| `...` | Spread                  |
| `->`  | Return type arrow       |
| `=>`  | Match arm arrow         |
| `?`   | Error propagation       |
| `as`  | Type cast               |

**Delimiters and punctuation:**

| Token | Name                    |
|-------|-------------------------|
| `(`   | Left parenthesis        |
| `)`   | Right parenthesis       |
| `{`   | Left brace              |
| `}`   | Right brace             |
| `[`   | Left bracket            |
| `]`   | Right bracket           |
| `,`   | Comma                   |
| `;`   | Semicolon               |
| `:`   | Colon                   |
| `::`  | Path separator          |
| `.`   | Field access / method call |

### 2.8 Token Precedence Rules

When the character stream is ambiguous, the lexer applies the **maximal munch**
rule: at each position, the longest possible token is produced. For example, `>>=`
is lexed as `>>` followed by `=`, not `>` followed by `>=`. The exception is
`::` which is always lexed as a single path separator token, never as two colons.

The `<` and `>` tokens in generic type argument lists (e.g., `Map<K, V>`) are
disambiguated by the parser, not the lexer. The lexer always produces `<` and
`>` as comparison operators.

The `|>` token is lexed as a single pipe operator by maximal munch. This is
unambiguous: no valid expression has `|` immediately followed by `>` without an
intervening operand (bitwise OR has lower precedence than comparison, so `a | > b`
is never valid).

---

