# ESC long-press Home

The trusted Weston policy owns the ESC gesture. Applications never receive the
binding key and do not run gesture timers. A press begins one compositor state
machine:

- release before 800 milliseconds sends exactly one Back action;
- remaining held for 800 milliseconds sends exactly one Home action;
- releasing after Home does not send Back.

The policy polls Weston's authoritative pressed-key array at a bounded
20-millisecond interval. It does not trust repeat events, wall-clock time or a
Shell-provided duration. Duplicate presses cannot extend the threshold. Loss of
the trusted Shell, keyboard or compositor cancels the pending gesture.

Home uses the existing authenticated System Shell protocol action and forces
the trusted full overlay before delivery. No protocol version change is
required. `KEY_HOME`, produced by `Fn+K`, remains unregistered as a global key
and continues to the foreground application.

Native tests lock short release, the exact threshold, duplicate press,
backwards time and cancellation behavior. The compositor profile locks the
timer, key ownership and image-build sources. Physical V0.6 acceptance still
must measure the threshold and confirm ESC short/long behavior in Home, standard
and immersive application states.
