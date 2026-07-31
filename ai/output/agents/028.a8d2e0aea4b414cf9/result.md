All work is complete, verified, and committed. Summary:

**Build/test status**: `npm run build` (vue-tsc type-check + vite build) passes clean with zero errors. `npm run test` passes all 21 tests (15 pre-existing `SshCommandDisplay` hover/copy tests + 5 new `ServiceConnector` filter/emit tests + 1 new per-item `-R`/`-L` command test). No running backend/dev-server was available in this environment, so the build's type-checking is the primary verification gate, as anticipated — no manual browser walkthrough was performed.

**What was built** (4 commits on `mane`):
1. `daf1abe` — `frontend/src/api/entities.ts`/`friends.ts` rewritten around `PortConfig`/`PortSubscription`/`SubscribableOwner`, computed `is_server`/`is_client`; new `frontend/src/portNames.ts` mirroring the backend's `guess_service_name`; `labels.ts`'s `entityTypeLabel` repurposed as `roleBadges()`.
2. `2d7084c` — `frontend/src/components/SshCommandDisplay.vue` rebuilt so direction is derived per item (owned services → `-R`, subscriptions → `-L`, both can appear together); old discovery-section UI and trailing ports-table removed.
3. `dfb1557` — `frontend/src/pages/EntityDetailPage.vue` rebuilt as one uniform layout (My services / My subscriptions / access-rule scope picker); new `frontend/src/components/ServiceConnector.vue` for the subscribe/browse UI with Show/Origin filters.
4. `4d784a6` — `frontend/src/pages/EntitiesPage.vue` merged into a single filterable list (`role` query param, 🖧/💻 badges, search box), `router/index.ts` updated for `/servers`/`/clients`.

**Deviations from the plan, with reasons**:
- Status-dot columns were omitted entirely (not stubbed) since no backend live-tracking exists yet — per the task's explicit instruction not to fake this.
- `EntityDetailResponse` doesn't carry the entity's own subscriptions directly, so `SshCommandDisplay`'s `-L` flags are fed from a flattened `ownSubscriptionRows` computed from the already-fetched `subscribable-services` response rather than a dedicated field — this matches the actual backend shape confirmed by reading `routes/entities.rs`.
- `ServiceConnector.vue` filtering is fully client-side over the single `subscribable-services` fetch, as the task allowed.
- `EntitiesPage.vue` was already a single shared component pre-filtered via a prop (not separate page files), so "merging" it mainly meant renaming `entityType`→`role`, adding the in-page filter chips/search, and swapping the badge column — there was no separate servers/clients page to delete.

Relevant files: `frontend/src/api/entities.ts`, `frontend/src/api/friends.ts`, `frontend/src/portNames.ts`, `frontend/src/labels.ts`, `frontend/src/stores/entities.ts`, `frontend/src/pages/FriendsPage.vue`, `frontend/src/components/SshCommandDisplay.vue` (+`.spec.ts`), `frontend/src/pages/EntityDetailPage.vue`, `frontend/src/components/ServiceConnector.vue` (+`.spec.ts`), `frontend/src/pages/EntitiesPage.vue`, `frontend/src/router/index.ts`.

result: Frontend restructure for port-configs/port-subscriptions is complete and committed in 4 checkpoints on `mane` (`daf1abe`, `2d7084c`, `dfb1557`, `4d784a6`) — merged Entities list with role badges/filters, rebuilt EntityDetailPage with My-services/My-subscriptions/access-scope sections, new ServiceConnector component, per-item -R/-L SshCommandDisplay — `npm run build` passes clean and all 21 frontend tests pass.