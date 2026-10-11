# Schedule controls

Starting with **0.7.35**, ordinary **Start**, **Restart** and **Start all** respect the enabled schedules assigned to a profile. Times use the backend host's local clock.

| Situation | Ordinary Start / Restart | Force mining |
| --- | --- | --- |
| Inside an assigned enabled window | Start/resume scheduled mining; stop when its last active window ends | Run without a time limit |
| Outside all assigned enabled windows | Resume scheduling and wait for the next window | Start immediately without a time limit |
| No enabled schedule assigned | Start immediately as a manual session | No separate override is needed |

For example, a daily **07:30–15:45** window starts mining at 07:30 and stops at 15:45. After **Stop → Start** at 10:00, the session still stops at 15:45. **Restart** rebuilds the process but keeps the scheduled end. At 16:00, **Resume schedule** waits for the next day's window. **Force mining** at 16:00 starts immediately and is labelled **forced · no time limit**.

**Resume schedule** returns a forced session to the planner. Inside a window it keeps the existing process and restores the scheduled end; outside a window it stops that process and waits. **Start all** reports separately how many profiles started and how many are waiting.

## Stopping and window boundaries

- **Stop** pauses the current dated occurrence, including overlapping or overnight windows. The scheduler will not immediately restart it. **Stop all** also pauses eligible stopped/failed profiles.
- A successful ordinary Start/Restart or Force mining clears that profile's pause. A later dated occurrence remains eligible automatically.
- Overlapping windows keep scheduled mining running until the last active one ends. Overnight windows belong to the day on which they start.
- End-of-window sleep/hibernate remains subject to the existing countdown and confirmation settings. It is blocked while another assigned window or a manual/forced miner is active. Tests use fake processes and never suspend a live mining rig.
- Closing a browser tab does not stop the Rust planner while its backend remains running. Exiting the owning desktop shuts down its managed miners.

Older versions treated all manually started/restarted sessions as unlimited. After installing 0.7.35, use **Start** or **Resume schedule** to return a running session to its schedule; verify the scheduled badge and the saved days/times.

## API behavior

Authenticated `POST /api/miners/:id/start`, `/restart`, and `POST /api/miners/start-all` use the same schedule-aware policy as the interface. `POST /api/miners/:id/force-start` is the explicit unlimited override. Successful per-profile responses include `mode` (`schedule`, `waiting`, or `manual`); a waiting response does not mean a miner was launched. Stop behavior and authentication are unchanged.
