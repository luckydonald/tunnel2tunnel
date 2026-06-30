---
name: russh_method_set
description: "russh 0.61 auth types are at the crate root, not russh::auth; entity.name is Option<String>"
metadata: 
  node_type: memory
  type: project
  originSessionId: 4e03ed8c-f268-407f-a241-415199236b9b
---

## russh auth types — import path

`russh::auth` is a private module. Import `MethodKind` and `MethodSet` from the crate root:

```rust
use russh::{MethodKind, MethodSet};
```

**Why:** Compiler gives `E0603: module auth is private` if you use `russh::auth::{…}`.

**How to apply:** Always use the crate-root path for these types when building `proceed_with_methods` in `Auth::Reject`.

## entity.name is Option<String>

`Entity.name` is `Option<String>`, so it cannot be used directly with the `%` formatter in `tracing::` macros.

```rust
// Wrong — compile error E0277
tracing::info!(entity_name = %entity.name, "…");

// Correct
tracing::info!(entity_name = entity.name.as_deref().unwrap_or("(unnamed)"), "…");
```

**Why:** `Option<String>` doesn't implement `Display`; tracing's `%` shorthand requires it.

**How to apply:** Any time an `Option<String>` field is logged with `%`, unwrap it first.
