In /home/user/git/luckydonald/tunnel2tunnel/frontend, I need context for adding toast notifications on websocket live status events.

1. Read composables/useLiveSocket.ts fully — what events/messages does it receive over the websocket, what payload shapes, and how do consumers currently subscribe to it.
2. Read pages/DashboardPage.vue fully — how it uses useLiveSocket, what "live status ring" / RemoteStatus stuff looks like there.
3. Search the whole frontend/src for any existing toast/notification library or component (grep for "toast", "notify", "Notification", "Snackbar", "Alert" in .vue/.ts files). Report if one exists (name, import path, API) or if none exists.
4. Check package.json for any installed toast libraries (vue-toastification, vue3-toastify, etc.)
5. Check labels.ts for how RemoteStatus / status enums are labeled (human readable names/colors) since toasts likely need similar text.
6. Check components/ dir listing for reusable patterns (e.g. existing global singleton components mounted in App.vue or AppShell).

Report: file paths+line numbers for key pieces, exact websocket message shapes/types (from useLiveSocket.ts), whether toast library exists already, and how App.vue/AppShell.vue is structured (any global overlay/portal mount point). Keep it factual and complete — this feeds a plan for adding toast notifications for live websocket events.