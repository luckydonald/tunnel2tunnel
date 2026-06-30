# Done

- [x] add a typeahead style dropdown to the port inputs, where you have common ports.
- [x] in the settings add a section with server information, including the SSH port to connect to, and the env var name.
- [x] in the ssh commands, make the server's SSH port clickable and lead to the settings.

# Misc

Create as separate commit (LPLP style):

- [ ] write a welcome message to the new ssh connection
  - something nerdy and MLP:FiM related.
- [ ] Regularly send a ping-like message
  - derpy mail mare themed
- [ ] Send message to all connected ends if another client/server/port etc.
  - client/server connects
  - port becomes available/unavailable
- [ ] have a cheap chat for the same clients:
  - A message typed into one session will be relayed to all others
  - Submit on enter
  - Prefixed with the friendly client/server name + uuid.
  - Something like `${sender.name} (${sender.uuid}): ${message}`.
- [ ] Clicking the green `-L <port>:<uuid>:<port>` command brings you to the port section of that server.
- [ ] Display the reversed access rules in the entities viewer as a secondary table.
- [ ] The _Access Rules_ table stays empty on initial page load.
