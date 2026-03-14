"""
Commute Scheduler – runs as a background asyncio task.

Every 60 seconds it checks all COMMUTE-type tasks across all users.
If the current time is within 30 minutes before a commute deadline,
it calls the directions service every 5 minutes and stores an alert
that the frontend can poll via /api/commute-alerts/{userid}.
"""

import asyncio
import json
import time
from datetime import datetime, timedelta
from collections import defaultdict
from typing import Optional

import ai_tools
import database


# ── In-memory notification store ──────────────────────────────────────
# { userid: [ { "commute_label": ..., "message": ..., "routes": ..., "ts": ... }, ... ] }
_alerts: dict[int, list[dict]] = defaultdict(list)

# Track when we last called directions per (userid, task_id) to enforce 5-min interval
_last_call: dict[tuple[int, str], float] = {}

# ── In-memory live trips (urgent one-time directions) ────────────────
# { trip_id: { "userid": int, "origin": str, "destination": str,
#              "deadline_ts": float, "created_ts": float, "label": str } }
_live_trips: dict[str, dict] = {}
_live_trip_counter = 0

ALERT_WINDOW_MINUTES = 30   # start alerting this many minutes before deadline
CALL_INTERVAL_SECONDS = 300  # call directions every 5 minutes
MAX_ALERTS_PER_USER = 20     # keep the list bounded


def get_alerts(userid: int) -> list[dict]:
    """Return and clear pending alerts for a user."""
    alerts = list(_alerts.get(userid, []))
    _alerts[userid] = []
    return alerts


def peek_alerts(userid: int) -> list[dict]:
    """Return pending alerts WITHOUT clearing them."""
    return list(_alerts.get(userid, []))


def _push_alert(userid: int, alert: dict):
    _alerts[userid].append(alert)
    # trim old alerts
    if len(_alerts[userid]) > MAX_ALERTS_PER_USER:
        _alerts[userid] = _alerts[userid][-MAX_ALERTS_PER_USER:]


def clear_user(userid: int):
    """Wipe all in-memory state for a user: alerts, live trips, and call timers."""
    _alerts.pop(userid, None)

    # Remove all live trips belonging to this user
    trip_ids = [tid for tid, t in _live_trips.items() if t["userid"] == userid]
    for tid in trip_ids:
        del _live_trips[tid]

    # Remove all _last_call entries for this user
    keys = [k for k in _last_call if k[0] == userid]
    for k in keys:
        del _last_call[k]

    print(f"[CommuteScheduler] Cleared all state for user {userid}")


def _is_in_alert_window(deadline_str: str, now: datetime) -> bool:
    """Check if `now` is within [deadline - 30min, deadline]."""
    try:
        h, m = map(int, deadline_str.split(":"))
        deadline_today = now.replace(hour=h, minute=m, second=0, microsecond=0)
        window_start = deadline_today - timedelta(minutes=ALERT_WINDOW_MINUTES)
        return window_start <= now <= deadline_today
    except Exception:
        return False


def _day_matches(days_csv: str, now: datetime) -> bool:
    """Check if today's day name is in the comma-separated list."""
    if not days_csv:
        # default: weekdays
        return now.weekday() < 5
    day_names = [d.strip().lower() for d in days_csv.split(",")]
    today = now.strftime("%A").lower()
    return today in day_names


_VEHICLE_ICON = {
    "BUS": "🚌",
    "SUBWAY": "🚇",
    "METRO_RAIL": "🚇",
    "RAIL": "🚆",
    "TRAM": "🚊",
    "COMMUTER_TRAIN": "🚆",
    "HIGH_SPEED_TRAIN": "🚄",
    "HEAVY_RAIL": "🚆",
    "LONG_DISTANCE_TRAIN": "🚆",
    "FERRY": "⛴️",
    "CABLE_CAR": "🚡",
    "FUNICULAR": "🚞",
    "SHARE_TAXI": "🚐",
    "TROLLEYBUS": "🚎",
}


def _format_commute_alert(label: str, routes: list[dict], deadline_str: str, now: datetime) -> str:
    """Build a human-readable alert message from parsed routes."""
    if not routes:
        return f"🚇 {label}: No transit routes found right now."

    fastest = routes[0]  # already sorted by duration

    # Detect if the route has any transit steps
    transit_steps = [s for s in fastest.get("steps", []) if s.get("travel_mode") == "TRANSIT"]
    walk_only = len(transit_steps) == 0

    lines = []
    lines.append(f"📍 {label.replace('_', ' ').title()}")

    # Duration header — only show dep/arr times when transit is involved
    dep = fastest.get('departure_time', '')
    arr = fastest.get('arrival_time', '')
    if dep and arr:
        lines.append(f"⏱ Total: {fastest['total_duration']}  (dep {dep} → arr {arr})")
    else:
        lines.append(f"⏱ Total: {fastest['total_duration']}")
    lines.append("")

    if walk_only:
        # Pure walking route
        lines.append(f"🚶 Walk the whole way – {fastest['total_duration']}")
        lines.append("No public transit needed for this distance.")
    else:
        # Build step-by-step itinerary
        for step in fastest.get("steps", []):
            mode = step.get("travel_mode", "")
            if mode == "WALKING":
                lines.append(f"🚶 Walk {step.get('duration', '')}")
            elif mode == "TRANSIT":
                vtype = step.get("vehicle_type", "")
                icon = _VEHICLE_ICON.get(vtype, "🚍")
                line_name = step.get("transit_line", "?")
                dep_stop = step.get("departure_stop", "")
                arr_stop = step.get("arrival_stop", "")
                dep_time = step.get("departure_time", "")
                arr_time = step.get("arrival_time", "")
                num_stops = step.get("num_stops", 0)

                time_range = f"  {dep_time} → {arr_time}" if dep_time and arr_time else ""
                lines.append(f"{icon} {line_name}{time_range}  ({step.get('duration', '')})")
                if dep_stop:
                    lines.append(f"     Board at {dep_stop}")
                if arr_stop:
                    stop_label = f"{num_stops} stop{'s' if num_stops != 1 else ''}" if num_stops else ""
                    lines.append(f"     Exit at {arr_stop}" + (f"  ({stop_label})" if stop_label else ""))
                lines.append("")

    # Time until deadline
    if deadline_str:
        try:
            h, m = map(int, deadline_str.split(":"))
            deadline_dt = now.replace(hour=h, minute=m, second=0, microsecond=0)
            mins_to_deadline = max(0, int((deadline_dt - now).total_seconds() / 60))
            if mins_to_deadline <= 5:
                lines.append(f"🔴 {mins_to_deadline} min left before your {deadline_str} deadline!")
            else:
                lines.append(f"🎯 {mins_to_deadline} min left before your {deadline_str} deadline")
        except Exception:
            pass

    # Show alternative routes summary if available
    if len(routes) > 1:
        alts = []
        for r in routes[1:3]:
            alt_transit = []
            for s in r.get("steps", []):
                if s.get("travel_mode") == "TRANSIT":
                    vt = s.get("vehicle_type", "")
                    ic = _VEHICLE_ICON.get(vt, "🚍")
                    alt_transit.append(f"{ic}{s.get('transit_line', '?')}")
            chain = " → ".join(alt_transit) if alt_transit else "🚶 Walk"
            dep_t = r.get('departure_time', '')
            dep_info = f", dep {dep_t}" if dep_t else ""
            alts.append(f"  {chain}  ({r['total_duration']}{dep_info})")
        lines.append("")
        lines.append("Other options:")
        lines.extend(alts)

    return "\n".join(lines)


# ── Live trip management ─────────────────────────────────────────────

def register_live_trip(userid: int, origin: str, destination: str,
                       minutes_until_deadline: int) -> str:
    """Register an urgent one-time trip. Returns the trip_id."""
    global _live_trip_counter
    _live_trip_counter += 1
    trip_id = f"live_{_live_trip_counter}"
    now = time.time()
    deadline_ts = now + (minutes_until_deadline * 60)
    deadline_dt = datetime.fromtimestamp(deadline_ts)

    _live_trips[trip_id] = {
        "userid": userid,
        "origin": origin,
        "destination": destination,
        "deadline_ts": deadline_ts,
        "created_ts": now,
        "label": f"🚨 Trip to {destination[:40]}",
        "deadline_str": deadline_dt.strftime("%H:%M"),
    }
    # Force immediate first call by NOT setting _last_call
    print(f"[LiveTrip] Registered {trip_id} for user {userid}: "
          f"{origin[:30]} → {destination[:30]} deadline in {minutes_until_deadline}min")
    return trip_id


def get_active_live_trips(userid: int) -> list[dict]:
    """Return all active live trips for a user."""
    now = time.time()
    return [
        {**trip, "trip_id": tid,
         "minutes_remaining": max(0, int((trip["deadline_ts"] - now) / 60))}
        for tid, trip in _live_trips.items()
        if trip["userid"] == userid and trip["deadline_ts"] > now
    ]


async def _check_live_trips():
    """Check all live trips, call directions every 5 min, expire when deadline passes."""
    now_ts = time.time()
    now = datetime.now()
    expired = []

    for trip_id, trip in _live_trips.items():
        # Expired?
        if now_ts > trip["deadline_ts"]:
            expired.append(trip_id)
            # Send a final "deadline passed" alert
            mins_ago = int((now_ts - trip["deadline_ts"]) / 60)
            _push_alert(trip["userid"], {
                "commute_label": trip["label"],
                "trip_id": trip_id,
                "message": f"⏰ {trip['label']} – deadline has passed ({mins_ago} min ago). Live tracking stopped.",
                "routes": [],
                "ts": now.isoformat(),
                "type": "live_trip_expired",
            })
            continue

        # Enforce 5-min interval
        key = (trip["userid"], trip_id)
        last = _last_call.get(key, 0)
        if now_ts - last < CALL_INTERVAL_SECONDS:
            continue

        _last_call[key] = now_ts
        mins_left = max(0, int((trip["deadline_ts"] - now_ts) / 60))

        try:
            raw_routes = await ai_tools.call_directions_service(
                trip["origin"], trip["destination"]
            )
            parsed = ai_tools.parse_transit_directions(raw_routes)
            message = _format_commute_alert(
                trip["label"], parsed, trip["deadline_str"], now
            )
            # Prepend urgency countdown
            message = f"⏳ {mins_left} min left until deadline\n{message}"

            _push_alert(trip["userid"], {
                "commute_label": trip["label"],
                "trip_id": trip_id,
                "message": message,
                "routes": parsed[:3],
                "ts": now.isoformat(),
                "type": "live_trip",
                "minutes_remaining": mins_left,
            })
            print(f"[LiveTrip] Alert pushed for {trip_id} – {mins_left}min remaining")
        except Exception as e:
            print(f"[LiveTrip] Directions failed for {trip_id}: {e}")
            _push_alert(trip["userid"], {
                "commute_label": trip["label"],
                "trip_id": trip_id,
                "message": f"⚠️ Could not fetch directions: {e}\n⏳ {mins_left} min left",
                "routes": [],
                "ts": now.isoformat(),
                "type": "live_trip",
            })

    # Clean up expired trips
    for tid in expired:
        del _live_trips[tid]
        # Also clean up _last_call entries
        keys_to_del = [k for k in _last_call if k[1] == tid]
        for k in keys_to_del:
            del _last_call[k]
        print(f"[LiveTrip] Expired and removed {tid}")


async def _check_commutes():
    """Single pass: check all commute tasks and fire alerts if needed."""
    try:
        if database.pool is None:
            return

        # Fetch all COMMUTE tasks across all users
        async with database.pool.acquire() as conn:
            rows = await conn.fetch(
                "SELECT * FROM public.task WHERE type = 'COMMUTE'"
            )

        now = datetime.now()

        for row in rows:
            row = dict(row)
            userid = row.get("userid")
            task_id = str(row.get("id"))
            payload = row.get("payload")
            if isinstance(payload, str):
                try:
                    payload = json.loads(payload)
                except Exception:
                    continue
            if not isinstance(payload, dict):
                continue

            label = payload.get("label", "commute")
            origin = payload.get("origin")
            destination = payload.get("destination")
            deadline = payload.get("deadline")
            days = payload.get("days", "")

            if not all([origin, destination, deadline]):
                continue

            # Check day and time window
            if not _day_matches(days, now):
                continue
            if not _is_in_alert_window(deadline, now):
                continue

            # Enforce 5-minute interval per commute
            key = (userid, task_id)
            last = _last_call.get(key, 0)
            if time.time() - last < CALL_INTERVAL_SECONDS:
                continue

            _last_call[key] = time.time()

            # Call directions service
            try:
                raw_routes = await ai_tools.call_directions_service(origin, destination)
                parsed = ai_tools.parse_transit_directions(raw_routes)
                message = _format_commute_alert(label, parsed, deadline, now)
                _push_alert(userid, {
                    "commute_label": label,
                    "task_id": task_id,
                    "message": message,
                    "routes": parsed[:3],  # top 3 routes
                    "ts": now.isoformat(),
                })
                print(f"[CommuteScheduler] Alert pushed for user {userid}: {label}")
            except Exception as e:
                print(f"[CommuteScheduler] Directions call failed for {label}: {e}")
                _push_alert(userid, {
                    "commute_label": label,
                    "task_id": task_id,
                    "message": f"⚠️ Could not fetch directions for {label}: {e}",
                    "routes": [],
                    "ts": now.isoformat(),
                })

    except Exception as e:
        print(f"[CommuteScheduler] Error in check loop: {e}")


async def run_scheduler():
    """Background loop – runs forever, checking every 60 seconds."""
    print("[CommuteScheduler] Started ✓")
    while True:
        await _check_commutes()
        await _check_live_trips()
        await asyncio.sleep(60)
