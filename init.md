/plan I want to implement a SSH tunnel mananger.
The general purpose is to write a server where both a "server" and a "client" can create SSH tunnels to connect with each other.
Basically a bit what a home VPN would do, but "rondevouz server" idea sprinked in, always available for firewall breaking, outgoing connections are easy.

So basically whatever plays server creates a tunnel, forwarding it's local port of whatever service running to the t2t manager via SSH (key auth, pub-key uploaded beforehand)
Now a client (also pub-key uploaded beforehand) ssh tunnel to the same t2t server, to get the port locally.
That's where it's different to normal port forwarding of our server to a cloud instance, where it would be all open and not secured in any way.
But we will use ssh login of the client's tunnel to make sure only allowed users can connect (via tunnel).
The rust thing would then shove data between tunnels, basically routing the vpn traffic based on the "ip/domain" given in the SSH connection string.


Here's what the user should be able to do:
1. at container start, an admin user & password is provided via env vars
    - provide `docker-compose.yml` and `.env.example` for selfhosting.
    - separate coolify `docker-compose.coolify.yml` config with magic env variables handles user and password creation etc. for us, easiest approach.
    - is stored to the db, hashed, salted and secure; that user gets the admin flag set to true.
2. a website is served
3. admin/user visits /login
    - other routes redirect there if not auth'd
4. admin/user logs in
5. Dashboard
6. Navigates to servers page
7. Clicks "New Server" at the top and bottom of all those servers. 
8. A form opens:
   - File field for selecting a pubkey for that server
     - if name is still empty, filename is pre-filled there
     - if description is still empty, pubkey comment (at the end of the pubkey line) is pre-filled 
   - name
   - optional description textarea
   - nullable IP whitelist
     - for now: textarea
     - basically advanced `.gitignore` for IP addresses:
       - normal ip addresses
       - allows subnet notion
       - simple `?`/`*` wildcards
       - regex (`s/…/<flags>`)
       - inversion (denying) with `!`
     - For now implement as TEXT in the db
     - rules are split by newlines and processed top to bottom
   - nullable valid-until-date.
   - hidden database fields:
     - `created_at`: timestamp with UTC timezone
     - `updated_at`: timestamp with UTC timezone
     - `deleted_at`: timestamp with UTC timezone (soft-delete)
9. Navigates to clients page
10. user/admin Uploads pubkey of a client, see **8.**
11. Navigates to servers page
12. Selects the created server (from **8.**) — clicking on "Clients" option next to "Edit".
13. Can now choose who to allow:
    1. all my devices (clients + servers) (current + future)
    2. selecting any of my clients
        - select all/none checkboxes to help you
        - includes an "all" option (current + future)
    3. selecting any of my servers
        - select all/none checkboxes to help you
        - includes an "all" option (current + future)
    4. other user's clients/servers
        - note that for that, the other user has to first allow you to see any or all of them, see below.
            - includes an "all" option (for allowing to see (and hence add) all of their clients/servers/both) (allow all) (current + future)
        - includes an "all" option (for all the other users clients/servers/both) (all devices) (current + future)
    5. public lite: any authenticated user (not recommended)
    6. public plus: any ssh key auth will be accepted and not checked (not recommended)
    7. public maxi: any ssh connection, key or without key, will be accepted (not recommended)
    8. public full: the ssh port of the server is directly shared. (not recommended)
    - For now, 1-4 have to be implemented, 5-7 are possibly a later feature.
    - For each of those you can also add a hostname (i.e. to be used in the ssh command as machine "ip/address")
14. Could also remove those added again.
15. Can invite another user to have access to this server
    - either via friend list or via user-lookup (exact username match)
    - slightly similar to the flow inviting users to a GitHub repo/org
16. with that has connected his own client to that server (selecting the client directly for now)
17. to confirm, navigates to the client list.
18. analog to **12.** there's edit & "Servers", the latter is clicked.
19. analog to **13. - 15.** but with "Servers" preferred in the GUI order instead.
20. Could also remove those added again.
21. Can invite another user to have access to this server
    - either via friend list or via user-lookup (exact username match)
    - slightly similar to the flow inviting users to a GitHub repo/org
22. with that has connected his own client to that server (selecting the client directly for now)
23. to confirm, navigates to the client list.
24. analog to **12.** there's edit & "Servers", the latter is clicked.
25. the server added in step **16.** is listed there as well, and could be removed again (analog to **14.**)
26. the user goes to his setting page
27. the user changes his password from the default.
    - the env var will not overwrite the admin's password after initial setup.
    - obviously has to input _old_, _new_ & _new (3)_ passwords, and hit save.
28. because of the password change, the page directly links the key quick-removal page or the server quick-removal page, or the purge-all-access page in case one/some/all of those are needed to be reset, too.
    - the purge page will disconnect all server-client, client-client, server-server, user-server, user-client, user-user permissions, and invalidate all ssh keys.
29. curiously, the user/admin clicks that link.
30. the admin/users sees a list of all his server keys and all his client keys in a compact view, allowing to _check/uncheck/check all/uncheck all_ for
    _both/server keys/client keys_, and submit.
31. user/admin selects all.
32. user/admin deselects the server created earlier.
33. user/admin hits "Delete Key" button (submit).
34. new page, with "Warning, the following key/keys will be deleted:"
35. user/admin hits cancel, which navigates back to the selection, still retaining the selection made before.
36. for now not needing to delete any key, the user/admin checks out the similar Entity quick-removal page.
37. that page lists Clients and Servers having access to some Clients or Servers of the user, including those shared from/to other users.
38. the same _select, confirm and remember selection on abort_ logic as **26.**-**30.** is tested.
39. pleased, the admin heads over to the "Users" page, which is not available to other users.
40. here, the admin can see all user accounts, their admin (aka. superuser) status, and edit any fields via "Edit" button. This includes resetting the password.
41. the admin can also promote or demote any account as admin
42. clicking your own demote button, styled danger/orange, is enabled as long as there is one other superuser remaining.
43. the admin's own button causes a "Are you really sure to unadmin yourself?" confirmation (danger/orange), explaining the consequences for dummies (but
    still with real words and sentences).
44. The demotion is made atomic with the other-admin-exists check on the DB side to not accidentially race condition here.
45. The admin clicks the "new user" button at the end of the list
46. Instead, the toggle at the bottom of the Users page which allows sign-ups (off per default, has env var, updates on boot from env var).
47. in the new opened form the admin can create a user with
    - username,
    - optional email,
    - optional description textarea,
    - optional password (will allow that user to use the user signup by entering that exact username (and if set, that exact email))
    - if the account is locked (default: no, that would be silly)
48. In that userlist there's a "+ Friend" button quickly leading you to the friend form, with that username pre-filled.
49. Thinking about that, you check your own "My Friends" section.
50. On the top you would have pending incoming invitations (from someone to you).
51. Any friends you have invited to a resource of yours appear here, with a summary of what they can access.
    - E.g.:
        - 43 clients · 1 server · no full access
        - no clients · all servers (123) · no full access
        - 1 client · 4 servers · no full access
        - full access (123 clients · 2 servers) <small>+ 2 clients</small> <small>+all servers</small>
    - second row of details for the other way around - what you can access from them.
    - card like design, those details are the bottom of that.
    - grayed out if no resource is shared yet
    - quick buttons to 
      - delete (with confirmation)
      - share one of my resources (select as in **13.1.**-**13.3**)
      - edit access
52. Satisfied for now, you go back to the "All Entities" page.
    - those are both servers and clients
    - they are the same on a database level, only a `type` field separates them to go into one of the frontend categories, to make things easier to manage and find.
    - this page is also just the same as **11.** and **18.**-**19.**, just not filtered for those two types.
      - so the order may be interleaving the types.
    - an icon displays which type it is on first glance.
    - you click on one of them to go to the detail page of it
      - (soft-)deleted one would be 404
      - displays
        - type (icon + text)
        - name
        - description
        - links to edit, access, etc. as usual.
        - the SSH command needed with the flags for port forwarding/tunneling
        - created at
        - last edit at
        - last 10 connections with timestamps (start + end)
          - merges multiple event types/tables:
            1. ssh connection (-attempts), including the following data:
               - if login succeeded
               - which ssh key was used,
               - ssh flags attempted,
               - what ports where shared/requeted to tunnel,
               - local/remote "ip/address" used
               - headers? is that a thing with ssh? Idk? env vars?
               - ...
            2. navigation to server details (including api access)
               - timestamp of requesting data
               - if within 5 minutes (env var), extend the end date of an previous log.
               - user
            3. frontend login attempts
               - including the wrong passwords
               - passwords hidden by default (5 dots)
          - client/server access (= establishing connection, with routing allowed to that server/client)


???. the user logs out in the UI via the logout button.


---

# Changes/Additions Round 1

- use the full name `tunnel2tunnel` everywhere. Only the binary may be called `t2t`.
- use `UUIDv7` everywhere
- Vue means Vue 3 script setup, fully typed TS (no any, no unknown), SCSS. 
- all dates in db shall be timezoned
- server shall be in UTC.
- every table shall have `created_at`, `updated_at`, automated db side.
  - can we D.R.Y. that? In python I'd use a mixin, I don't know about rust.
- Frontend is SPA with sane chunk splitting
- why no `CHECK` for `friendships.status`?
- if an entity's name is not set, UI etc. shall display the UUID as `<code>…<code>`
  - probably best to have a small component with slot for consistent styling of an entity across the app?
- "pubkey upload" is just vue parsing the file, have textarea with file input, also filling the textarea.
  - check for plausibility of it being a textfile and not too long before dumping it into the textarea.
- server/clients can have type icon, too, so it's just different filters, otherwise same page component.
- `friendships`: not sure about `can_see_clients BOOL, can_see_servers BOOL, can_see_all BOOL`:
  - like clients + servers = all would be redundant
  - I don't see the case covered where I would allpow someone access to only one of my servers?
  - Maybe I confused myself with `friendship ` vs `access`?
    - try to explain clearly with your words.
- I would like to separate SSH Key from `entity`
- logins (success/failure) are not hidden, only the passwords are (6 dots)
- always record if it's successful authorization or failure, so we can impose `fail2ban` like blocking.
  - to actually integrate real `fail2ban`, allow setting an env var for a file to write to.
    - at least for ssh, you can imitate SSH authentication log, example could point to `/var/log/auth.log`.
  - for the other services, either reuse some good standard format, or provide a filter definition. (i.e. for placing at `/etc/fail2ban/filter.d/….conf`)
- `target-hostname` may also be the `entity`s UUIDv7, the user still needs to have access in some way though.
- A server shall have a way to define its ports.
  - if multiple, the `-L` part needs to be repeated.
  - the command generator shall be interactive, hovering the ports would highlight the entry in the ports table, and vice versa.
  - they shall be editable in the table, and so they would change in the ssh prompt.
  - table would be
    - `| **enabled** | **local port** | **proxy port** | **name** | **description** | |`
    - `| quick way to turn a forwarded port on or off | the port on the server machine | the port internal to <code>tunnel2tunnel</code> | short | free text for your notes | |`
    - then the rows: 
    - `| checkbox | numeric | numeric | text | textarea |`
  - put the SSH command generation earlier, as I need it to test the SSH connection sharing.
  - the first (zeroth?) phase should be a very simple ssh-to-ssh tunnel test server. 
    - This is to test feasibility.
    - No auth, just passwordless ssh tunnel.
    - One "host" in the connection command will be `client`, one `server`
      - Assume only 1 connection per slot
      - connect those two.
