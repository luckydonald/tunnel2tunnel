# Done

- [x] add a typeahead style dropdown to the port inputs, where you have common ports.
- [x] in the settings add a section with server information, including the SSH port to connect to, and the env var name.
- [x] in the ssh commands, make the server's SSH port clickable and lead to the settings.
- [x] write a welcome message to the new ssh connection
  - something nerdy and MLP:FiM related.
- [x] Regularly send a ping-like message
  - derpy mail mare themed
- [x] Send message to all connected ends if another client/server/port etc.
  - client/server connects
  - port becomes available/unavailable
- [x] have a cheap chat for the same clients:
  - A message typed into one session will be relayed to all others
  - Submit on enter
  - Prefixed with the friendly client/server name + uuid.
  - Something like `${sender.name} (${sender.uuid}): ${message}`.
- [x] Clicking the green `-L <port>:<uuid>:<port>` command brings you to the port section of that server.
- [x] Display the reversed access rules in the entities viewer as a secondary table.
- [x] The _Access Rules_ table stays empty on initial page load.

10 done.

# Misc

Implement as separate commit (see @ai/skills/commit-with-lplp-style/SKILL.md) for each task:

- [x] The link in the `Incoming Grants` change the URL correctly, but the page stays as is. So the router failes to switch or something.
- [x] For the serverside, allow to set a different host than `localhost`, i.e. for sharing a different computer's access. Table default shall stay `localhost`.
- [x] Access Rules should be links to the entities as well.
- [x] Don't use `alert()` to do frontend error reporting (especially failed network requests), use toasts and sentry logging instead.

