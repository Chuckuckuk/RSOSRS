# CLAUDE.md

Guidance for Claude Code in this repository. The codebase itself is documented in [`docs/`](docs/README.md).

## Working with GitHub issues

Issues live at <https://github.com/Chuckuckuk/RSOSRS/issues>. Use `gh` with `-R Chuckuckuk/RSOSRS`.

### Filing an issue

- File problems found while working: bugs outside the current change, follow-up features, refactors worth doing later. Link them in your summary.
- Start the title with the prefix of its type (see [Title prefixes](#title-prefixes)).
- Say what happens now, what should happen, and where in the code. Name the branch it was found on.
- Always add labels; see [Labels](#labels).
- Map out its relationships; see [Relationships](#relationships).

### Relationships

Set relationships on GitHub with `gh issue edit` whenever an issue is filed or its scope changes:

- **Parent and sub-issues.** A feature with several follow-ups gets a parent issue (e.g. #29 Woodcutting over trees, sounds, pets and roll timing; #2 doors over sounds, more doors and gate layout).
  - The parent is titled after the feature, e.g. `[feature] Woodcutting`.
  - Its description says what is done (with the PR) and lists what remains.
  - `gh issue edit <parent> -R Chuckuckuk/RSOSRS --add-sub-issue 6,7,9`, or `--parent <n>` from the child.
  - An issue has only one parent. Nest a narrower issue under the broader one (#8 regular-log trees under #16 all trees under #29 Woodcutting), not under both.
  - When one issue's leftovers become new issues, make them its sub-issues (#25–#27 under #18).
- **Blocked by.** Use `--add-blocked-by <n>` when an issue cannot be done or merged before another: it builds on unmerged work (#18 on #2), or needs a mechanism another issue adds (#27 curtains needs #20 blocking data).
- **Related only.** An issue that applies to more than the feature (#5 XP drops and #13 ground items are for every skill) is not made a sub-issue. Name it under "Related" in the parent's description instead.
- **Overlapping issues.** When two issues touch the same area without one containing the other, say in each how they split the work (a comment on the older one), as #8 and #16 do.

### Title prefixes

Each issue gets exactly one prefix, and it matches the issue's type label one-to-one:

| Prefix | Type label | Use for |
|---|---|---|
| `[bug]` | `bug` | Something is broken |
| `[feature]` | `enhancement` | New gameplay or UI |
| `[parity]` | `parity` | Works, but does not match the real game (timing, messages, rates) |
| `[refactor]` | `refactor` | Restructuring with no change in behavior |
| `[research]` | `research` | Check the real game or the wiki before building |
| `[tooling]` | `tooling` | Dev hooks, tests, build, CI |
| `[docs]` | `documentation` | Docs only |

If an issue's type changes, update its prefix and type label together.

### Starting work on an issue

1. Assign it to the user: `gh issue edit <n> -R Chuckuckuk/RSOSRS --add-assignee @me`.
2. Link it to its branch: `gh issue develop <n> -R Chuckuckuk/RSOSRS --name <branch> --base develop`.
   - If the branch already exists locally, also set its upstream: `git fetch origin && git branch --set-upstream-to=origin/<branch> <branch>`.
3. Check the link: `gh issue develop --list <n> -R Chuckuckuk/RSOSRS`.
4. If the work builds on a branch that is not merged yet, stack on it: `--base <that branch>` in step 2, mark the issue blocked by the other one, and open the pull request (into `develop`) only once the base has merged, or as a draft that says it depends on it.

Branching: each feature branch is cut from `develop` and merges back into `develop` by pull request. `develop` is merged into `main` separately; never open a feature PR against `main`.

## Labels

The [label dictionary](#label-dictionary) below is the source of truth for the labels we use. Check it whenever you create or update an issue:

1. Apply every label in the dictionary that fits.
2. Only if none fits, add a new label. Create it on GitHub with a description (`gh label create <name> -R Chuckuckuk/RSOSRS --color <hex> --description "..."`) and add it to the dictionary in the same change.
3. Never apply a label that is not in the dictionary. If GitHub has one that the dictionary lacks, add it to the dictionary first, or leave it unused.

Every issue gets:
- exactly one **type** label, the one that matches its title prefix;
- at least one **area** label;
- any **topic** labels that describe it, and **triage** or **resolution** labels when they apply.

### Label dictionary

| Label | Group | Use for |
|---|---|---|
| `bug` | type | Something isn't working (`[bug]`) |
| `enhancement` | type | New feature or request (`[feature]` issues) |
| `refactor` | type | Restructure code without changing behavior (`[refactor]`) |
| `parity` | type | Works, but does not match the real game (timing, messages, rates) (`[parity]`) |
| `research` | type | Needs checking against the real game or the wiki before building (`[research]`) |
| `tooling` | type | Dev tools, test harness, debug hooks, build (`[tooling]`) |
| `documentation` | type | Improvements or additions to documentation (`[docs]`) |
| `area: sim` | area | Server and game rules (`osrs_core` library: `sim`, `rules`, `data`) |
| `area: client` | area | Game client and rendering (`osrs_core` binary, `osrs_render`) |
| `area: cache` | area | Cache decoding (`osrs_cache`) |
| `area: data` | area | JSON game data (`osrs_core/data`) |
| `combat` | topic | Combat rules: attacks, reactions, prayers, boss and NPC encounters |
| `skilling` | topic | Skills, XP, tools, resources, skilling pets |
| `scenery` | topic | World objects: trees, doors, stumps, object interactions |
| `interface` | topic | In-game UI: tabs, chatbox, overlays, click feedback |
| `audio` | topic | Sound effects and music |
| `accessibility` | topic | Barrier affecting people with disabilities |
| `good first issue` | triage | Good for newcomers |
| `help wanted` | triage | Extra attention is needed |
| `question` | triage | Further information is requested |
| `duplicate` | resolution | This issue or pull request already exists |
| `invalid` | resolution | This doesn't seem right |
| `wontfix` | resolution | This will not be worked on |

### Updating issues
Query issues whenever you suspect an issue needs updating. It may need a reworked description, new relationships, milestones, etc. If you update the description, add a comment describing the changes and the reasoning behind it.


## Working with GitHub Pull Requests

Use the gh CLI tool for PR operations.

### Creating

Generate a simple changelog using feat/chore/fix bullet points. Put a short description at the top. Make sure any issues have been linked. The PR will always target the `develop` branch.