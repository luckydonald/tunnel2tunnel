# AI query log file

#### General AI development guidelines:
- Create `ai/PROGRESS.md`, and keep it updated when you complete steps.
- You may refer to `ai/refrences` for code examples of other plugins or extra documentation provided for this task.
- When writing code, follow these guidelines:
  - Always prefer the early-return pattern to reduce nesting of `if`s, etc.
  - Similarly, prefer `if …` -> `continue`/`return`/`break` early in loops over large nested blocks.
- _If_ the project requires a frontend, use Vue, TS, and SCSS for that.
  - Prefer using `<script setup lang="ts">` style single file components.
  - Use proper TypeScript type hinting.
- _If_ the project requires a backend, use modern Python `3.14+` for that.
  - Do proper type hinting with full type annotations.
  - For type-hinting, prefer the native types (e.g. `dict[str, int]` over the older `typing.*` aliases like `Dict[AnyStr, int]`)
  - Prefer async programming where possible.
  - For web stuff: `FastApi`
  - For postgres: typed `sqlalchemy`
    - For migrations: `alembic`
- Write tests for both frontend and backend parts.
- Remember to update the `/CHANGELOG.md` and `/README.md` if existent (including other pre-existing documentation).
- If you want to write Markdown summaries of the task you just did (only if specifically asked for by the user!) write those to `ai/summaries/` folder, and never into the root folder.
  - However, usually you don't need to write Markdown summaries.
- Please prefer to use the read file tool over weird constructs with `cat` etc. Terminal should not be needed for searches most of the time, either.

----

#### Previous user prompts:

❯ @init.md

> What language/framework for the backend?
> - Rust (Axum + SQLx) (Recommended)
> - Python (FastAPI + SQLAlchemy)
> - Go (standard lib / chi)
> - Node.js (Fastify / Hono)
> 
> What for the web frontend?
> - Server-side HTML (Askama/Tera + HTMX) (Recommended)
> - React (Vite + TanStack Router)
> - Svelte / SvelteKit
> 
> What database?
> - SQLite (libsql/turso) (Recommended)
> - PostgreSQL
> 
> How should the actual SSH server be implemented?
> - russh crate (pure Rust SSH) (Recommended)
> - Wrap OpenSSH (authorized_keys + ForceCommand)
> - Decide later
> → Rust (Axum + SQLx) (Recommended)
>   VueJS
>   PostgreSQL
>   russh crate (pure Rust SSH) (Recommended)
> ```json
> {
>   "questions": [
>     {
>       "question": "What language/framework for the backend?",
>       "header": "Backend",
>       "options": [
>         {
>           "label": "Rust (Axum + SQLx) (Recommended)",
>           "description": "Aligns with the spec's mention of Rust for tunnel routing; single binary, high perf, compile-time safety."
>         },
>         {
>           "label": "Python (FastAPI + SQLAlchemy)",
>           "description": "Faster to prototype, big ecosystem, but separate process from tunnel routing code."
>         },
>         {
>           "label": "Go (standard lib / chi)",
>           "description": "Good for network services, easy concurrency, simple deployment."
>         },
>         {
>           "label": "Node.js (Fastify / Hono)",
>           "description": "Large ecosystem, easy full-stack if sharing types with frontend."
>         }
>       ],
>       "multiSelect": false
>     },
>     {
>       "question": "What for the web frontend?",
>       "header": "Frontend",
>       "options": [
>         {
>           "label": "Server-side HTML (Askama/Tera + HTMX) (Recommended)",
>           "description": "Minimal JS, pairs naturally with Rust/Go backends, fast to ship, no build step for the frontend."
>         },
>         {
>           "label": "React (Vite + TanStack Router)",
>           "description": "Rich interactivity, component ecosystem, requires separate build step."
>         },
>         {
>           "label": "Svelte / SvelteKit",
>           "description": "Lightweight, compiles to vanilla JS, good DX."
>         }
>       ],
>       "multiSelect": false
>     },
>     {
>       "question": "What database?",
>       "header": "Database",
>       "options": [
>         {
>           "label": "SQLite (libsql/turso) (Recommended)",
>           "description": "Zero-ops, single file, perfect for self-hosted containers. Can migrate to Postgres later."
>         },
>         {
>           "label": "PostgreSQL",
>           "description": "Production-grade, great for multi-instance deployments, requires separate container."
>         }
>       ],
>       "multiSelect": false
>     },
>     {
>       "question": "How should the actual SSH server be implemented?",
>       "header": "SSH layer",
>       "options": [
>         {
>           "label": "russh crate (pure Rust SSH) (Recommended)",
>           "description": "Embed the SSH server directly in the Rust binary; full control over auth and tunnel routing."
>         },
>         {
>           "label": "Wrap OpenSSH (authorized_keys + ForceCommand)",
>           "description": "Delegate auth to OpenSSH, intercept via ForceCommand script. Less code, less control."
>         },
>         {
>           "label": "Decide later",
>           "description": "Plan the web app first; leave SSH layer as a stub/interface."
>         }
>       ],
>       "multiSelect": false
>     }
>   ],
>   "answers": {
>     "What language/framework for the backend?": "Rust (Axum + SQLx) (Recommended)",
>     "What for the web frontend?": "VueJS",
>     "What database?": "PostgreSQL",
>     "How should the actual SSH server be implemented?": "russh crate (pure Rust SSH) (Recommended)"
>   },
>   "annotations": {}
> }
> ```
> 

