Find where tarpit/trap trigger threshold (number of failed login attempts before trapping) is configured and enforced in this codebase. Look in crates/tunnel2tunnel-ssh/src/tarpit/mod.rs and crates/tunnel2tunnel-ssh/src/lib.rs, and any related settings/config models, DB migrations, frontend Settings page. Report: 
1. Exact field/variable name for the threshold, its type, default value, where stored (DB column, config struct).
2. The exact comparison logic (e.g. `attempts >= threshold`, `attempts > threshold`) with file:line.
3. Whether 0 is currently a valid/allowed value, and what happens if set to 0 (would first attempt already trap, or is there an off-by-one).
4. Any validation (min/max) on this setting in backend or frontend.
5. Any existing tests exercising boundary values (0, 1) for this setting.
Be precise with file:line citations. Keep report under 400 words.